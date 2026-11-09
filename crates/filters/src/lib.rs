//! Stateful, bounded custom motion processing. No device or output access.
use anyhow::{bail, Context, Result};
use rhai::{Array, CallFnOptions, Dynamic, Engine, Map, OptimizationLevel, Scope, AST, FLOAT, INT};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

pub const MAX_FILTERS: usize = 4;
pub const MAX_SOURCE: usize = 32 * 1024;
const MAX_OPERATIONS: u64 = 4096;
const MAX_CALL_TIME: Duration = Duration::from_millis(5);
const MAX_RATE: f64 = 1_000_000.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    RawGyro,
    #[default]
    BeforeBuiltin,
    AfterBuiltin,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FilterConfig {
    pub api_version: u32,
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub requires_accel: bool,
    pub stage: Stage,
    pub source: String,
    pub params: BTreeMap<String, f64>,
}
impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            api_version: 1,
            id: String::new(),
            name: String::new(),
            enabled: true,
            requires_accel: false,
            stage: Stage::default(),
            source: String::new(),
            params: BTreeMap::new(),
        }
    }
}
impl FilterConfig {
    pub fn validate(&self) -> Result<()> {
        if self.api_version != 1 {
            bail!("Unsupported filter API version: {}", self.api_version);
        }
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        {
            bail!("Filter id must contain 1–64 ASCII letters, digits, underscores or hyphens");
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 100 {
            bail!("Filter name must contain 1–100 characters");
        }
        if self.source.len() > MAX_SOURCE {
            bail!("Filter source exceeds 32 KiB");
        }
        if self.params.len() > 32 {
            bail!("A filter can have at most 32 numeric parameters");
        }
        for (key, value) in &self.params {
            if key.is_empty()
                || key.len() > 48
                || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                || !value.is_finite()
                || value.abs() > 1_000_000.
            {
                bail!("Invalid filter parameter: {key}");
            }
        }
        Ok(())
    }
    fn same_processor(&self, other: &Self) -> bool {
        self.enabled == other.enabled
            && self.stage == other.stage
            && self.requires_accel == other.requires_accel
            && self.source == other.source
            && self.params == other.params
            && self.api_version == other.api_version
    }
}
pub fn validate_configs(configs: &[FilterConfig]) -> Result<()> {
    if configs.len() > MAX_FILTERS {
        bail!("At most {MAX_FILTERS} custom filters are allowed");
    }
    let mut ids = BTreeSet::new();
    for config in configs {
        config.validate()?;
        if !ids.insert(&config.id) {
            bail!("Duplicate filter id: {}", config.id);
        }
        if config.enabled {
            static VALID: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
            let cache = VALID.get_or_init(Mutex::default);
            if !cache
                .lock()
                .map_err(|_| anyhow::anyhow!("Filter validation cache unavailable"))?
                .contains(&config.source)
            {
                let _ = RhaiProcessor::new(config)
                    .with_context(|| format!("Filter {}", config.name))?;
                let mut cached = cache
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Filter validation cache unavailable"))?;
                if cached.len() >= 32 {
                    cached.pop_first();
                }
                cached.insert(config.source.clone());
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub value: [f64; 3],
    pub raw: [f64; 3],
    pub accel_g: Option<[f64; 3]>,
    pub dt: f64,
    pub host_ns: u64,
    pub sensor_ns: u64,
}
impl Sample {
    fn map(self, stage: Stage) -> Map {
        let mut map = Map::new();
        for (key, value) in ["x", "y", "z"].into_iter().zip(self.value) {
            map.insert(key.into(), value.into());
        }
        for (key, value) in ["raw_x", "raw_y", "raw_z"].into_iter().zip(self.raw) {
            map.insert(key.into(), value.into());
        }
        for (key, value) in ["accel_x", "accel_y", "accel_z"]
            .into_iter()
            .zip(self.accel_g.unwrap_or([0.; 3]))
        {
            map.insert(key.into(), value.into());
        }
        map.insert("accel_fresh".into(), self.accel_g.is_some().into());
        map.insert("dt".into(), self.dt.into());
        map.insert(
            "host_ns".into(),
            INT::try_from(self.host_ns).unwrap_or(INT::MAX).into(),
        );
        map.insert(
            "sensor_ns".into(),
            INT::try_from(self.sensor_ns).unwrap_or(INT::MAX).into(),
        );
        map.insert(
            "stage".into(),
            match stage {
                Stage::RawGyro => "raw_gyro",
                Stage::BeforeBuiltin => "before_builtin",
                Stage::AfterBuiltin => "after_builtin",
            }
            .into(),
        );
        map.insert(
            "components".into(),
            (if stage == Stage::RawGyro {
                3 as INT
            } else {
                2 as INT
            })
            .into(),
        );
        map
    }
}

/// Implementations only transform timed values; input/output ownership remains
/// in Motion Studio. A future backend can implement this same contract.
pub trait SampleProcessor: Send {
    fn process(&mut self, sample: Sample, operations: u64) -> Result<[f64; 3]>;
    fn last_operations(&self) -> u64;
    fn reset(&mut self) -> Result<()>;
}

struct RhaiProcessor {
    engine: Engine,
    ast: AST,
    scope: Scope<'static>,
    state: Dynamic,
    params: Map,
    stage: Stage,
    epoch: Instant,
    deadline: Arc<AtomicU64>,
    operations: Arc<AtomicU64>,
}
impl RhaiProcessor {
    fn new(config: &FilterConfig) -> Result<Self> {
        let mut engine = Engine::new();
        engine.set_optimization_level(OptimizationLevel::None);
        engine.disable_symbol("eval");
        engine
            .set_max_operations(MAX_OPERATIONS)
            .set_max_call_levels(12)
            .set_max_variables(96)
            .set_max_functions(32)
            .set_max_expr_depths(32, 32)
            .set_max_array_size(128)
            .set_max_map_size(48)
            .set_max_string_size(1024);
        // Never let print/debug corrupt the worker's framed stdout protocol.
        engine.on_print(|_| {}).on_debug(|_, _, _| {});
        let epoch = Instant::now();
        let deadline = Arc::new(AtomicU64::new(u64::MAX));
        let clock = Arc::clone(&deadline);
        let operations = Arc::new(AtomicU64::new(0));
        let progress = Arc::clone(&operations);
        engine.on_progress(move |ops| {
            progress.store(ops, Ordering::Relaxed);
            if ops % 32 == 0
                && epoch.elapsed().as_nanos() > u128::from(clock.load(Ordering::Relaxed))
            {
                Some("Filter time budget exceeded".into())
            } else {
                None
            }
        });
        static AST_CACHE: OnceLock<Mutex<BTreeMap<String, AST>>> = OnceLock::new();
        let cache = AST_CACHE.get_or_init(Mutex::default);
        let cached = cache
            .lock()
            .map_err(|_| anyhow::anyhow!("Filter compilation cache unavailable"))?
            .get(&config.source)
            .cloned();
        let ast = if let Some(ast) = cached {
            ast
        } else {
            let ast = engine
                .compile(&config.source)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            if !ast
                .iter_functions()
                .any(|function| function.name == "process" && function.params.len() == 2)
            {
                bail!("Define fn process(sample, params) returning [x, y, z]");
            }
            let mut cached = cache
                .lock()
                .map_err(|_| anyhow::anyhow!("Filter compilation cache unavailable"))?;
            if cached.len() >= 32 {
                cached.pop_first();
            }
            cached.insert(config.source.clone(), ast.clone());
            ast
        };
        let mut result = Self {
            engine,
            ast,
            scope: Scope::new(),
            state: Map::new().into(),
            params: config
                .params
                .iter()
                .map(|(key, value)| (key.clone().into(), (*value).into()))
                .collect(),
            stage: config.stage,
            epoch,
            deadline,
            operations,
        };
        result.reset()?;
        Ok(result)
    }
    fn arm(&self) {
        self.deadline.store(
            (self.epoch.elapsed() + MAX_CALL_TIME)
                .as_nanos()
                .min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
    }
    fn validate_state(&self) -> Result<()> {
        let mut remaining = 512;
        data_size(&self.state, 0, &mut remaining)?;
        for (_, _, value) in self.scope.iter_raw() {
            data_size(value, 0, &mut remaining)?;
        }
        Ok(())
    }
}
fn data_size(value: &Dynamic, depth: usize, remaining: &mut usize) -> Result<()> {
    if depth > 6 || *remaining == 0 {
        bail!("Filter state is too large or deeply nested");
    }
    *remaining -= 1;
    if value.is_unit() || value.is::<bool>() || value.is::<INT>() {
        return Ok(());
    }
    if value.is::<FLOAT>() {
        if !value.clone_cast::<FLOAT>().is_finite() {
            bail!("Filter state contains a non-finite number");
        }
        return Ok(());
    }
    if value.is_string() {
        return Ok(());
    }
    if value.is_array() {
        for item in value
            .read_lock::<Array>()
            .context("Invalid filter array")?
            .iter()
        {
            data_size(item, depth + 1, remaining)?;
        }
        return Ok(());
    }
    if value.is_map() {
        for item in value
            .read_lock::<Map>()
            .context("Invalid filter map")?
            .values()
        {
            data_size(item, depth + 1, remaining)?;
        }
        return Ok(());
    }
    bail!("Filter state supports only numbers, booleans, strings, arrays and maps")
}
fn number(value: Dynamic) -> Result<f64> {
    let value = if value.is::<FLOAT>() {
        value.cast::<FLOAT>()
    } else if value.is::<INT>() {
        value.cast::<INT>() as f64
    } else {
        bail!("Filter output must contain three numbers");
    };
    if !value.is_finite() || value.abs() > MAX_RATE {
        bail!("Filter output is not finite or exceeds the motion range");
    }
    Ok(value)
}
impl SampleProcessor for RhaiProcessor {
    fn process(&mut self, sample: Sample, operations: u64) -> Result<[f64; 3]> {
        self.engine
            .set_max_operations(operations.clamp(1, MAX_OPERATIONS));
        self.operations.store(0, Ordering::Relaxed);
        self.arm();
        let started = Instant::now();
        let values = self
            .engine
            .call_fn_with_options::<Array>(
                CallFnOptions::new()
                    .eval_ast(false)
                    .bind_this_ptr(&mut self.state),
                &mut self.scope,
                &self.ast,
                "process",
                (sample.map(self.stage), self.params.clone()),
            )
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if started.elapsed() > MAX_CALL_TIME {
            bail!("Filter time budget exceeded");
        }
        if values.len() != 3 {
            bail!("Filter output must contain exactly [x, y, z]");
        }
        let mut out = [0.; 3];
        for (target, value) in out.iter_mut().zip(values) {
            *target = number(value)?;
        }
        self.validate_state()?;
        Ok(out)
    }
    fn reset(&mut self) -> Result<()> {
        self.engine.set_max_operations(MAX_OPERATIONS);
        self.scope = Scope::new();
        self.state = Map::new().into();
        self.arm();
        let _ = self
            .engine
            .eval_ast_with_scope::<Dynamic>(&mut self.scope, &self.ast)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        self.validate_state()
    }
    fn last_operations(&self) -> u64 {
        self.operations.load(Ordering::Relaxed).saturating_add(1)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilterStatus {
    pub id: String,
    pub name: String,
    pub stage: Stage,
    pub enabled: bool,
    pub bypassed: bool,
    pub error: Option<String>,
    pub calls: u64,
    pub last_us: u64,
    pub max_us: u64,
}
struct Entry {
    config: FilterConfig,
    processor: Option<Box<dyn SampleProcessor>>,
    status: FilterStatus,
}
#[derive(Default)]
pub struct FilterChain {
    entries: Vec<Entry>,
    stamp: Option<(u64, u64)>,
    operations_left: u64,
}
impl FilterChain {
    pub fn new(configs: &[FilterConfig]) -> Result<Self> {
        let mut chain = Self::default();
        chain.reconfigure(configs)?;
        Ok(chain)
    }
    pub fn reconfigure(&mut self, configs: &[FilterConfig]) -> Result<()> {
        validate_configs(configs)?;
        let mut replacements = BTreeMap::new();
        for config in configs {
            if config.enabled
                && !self.entries.iter().any(|entry| {
                    entry.config.id == config.id && entry.config.same_processor(config)
                })
            {
                replacements.insert(
                    config.id.clone(),
                    Box::new(
                        RhaiProcessor::new(config)
                            .with_context(|| format!("Filter {}", config.name))?,
                    ) as Box<dyn SampleProcessor>,
                );
            }
        }
        let mut old = std::mem::take(&mut self.entries);
        for config in configs {
            if let Some(index) = old.iter().position(|entry| {
                entry.config.id == config.id && entry.config.same_processor(config)
            }) {
                let mut entry = old.remove(index);
                entry.config = config.clone();
                entry.status.name = config.name.clone();
                self.entries.push(entry);
            } else {
                let status = FilterStatus {
                    id: config.id.clone(),
                    name: config.name.clone(),
                    stage: config.stage,
                    enabled: config.enabled,
                    bypassed: false,
                    error: None,
                    calls: 0,
                    last_us: 0,
                    max_us: 0,
                };
                self.entries.push(Entry {
                    config: config.clone(),
                    processor: replacements.remove(&config.id),
                    status,
                });
            }
        }
        Ok(())
    }
    /// A new FIFO sample owns a fresh shared budget, even when hardware clocks
    /// repeat. Call once before traversing its processing stages.
    pub fn begin_sample(&mut self) {
        self.stamp = None;
    }
    pub fn process(&mut self, stage: Stage, mut sample: Sample) -> [f64; 3] {
        let stamp = (sample.host_ns, sample.sensor_ns);
        if self.stamp != Some(stamp) {
            self.stamp = Some(stamp);
            self.operations_left = MAX_OPERATIONS;
        }
        for entry in &mut self.entries {
            if !entry.config.enabled || entry.config.stage != stage || entry.status.bypassed {
                continue;
            }
            let Some(processor) = &mut entry.processor else {
                continue;
            };
            if self.operations_left == 0 {
                break;
            }
            let started = Instant::now();
            let result = processor.process(sample, self.operations_left);
            self.operations_left = self
                .operations_left
                .saturating_sub(processor.last_operations());
            let elapsed = started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
            entry.status.calls = entry.status.calls.saturating_add(1);
            entry.status.last_us = elapsed;
            entry.status.max_us = entry.status.max_us.max(elapsed);
            match result {
                Ok(out) => sample.value = out,
                Err(error) => {
                    entry.status.bypassed = true;
                    entry.status.error = Some(format!("{error:#}").chars().take(2048).collect());
                }
            }
        }
        sample.value
    }
    pub fn reset(&mut self) {
        self.stamp = None;
        for entry in &mut self.entries {
            entry.status.bypassed = false;
            entry.status.error = None;
            entry.status.calls = 0;
            entry.status.last_us = 0;
            entry.status.max_us = 0;
            if let Some(processor) = &mut entry.processor {
                if let Err(error) = processor.reset() {
                    entry.status.bypassed = true;
                    entry.status.error = Some(format!("{error:#}"));
                }
            }
        }
    }
    /// Timestamp gaps clear healthy history while keeping a failed filter
    /// bypassed. A transient reset must not retry faulty code on every sample.
    pub fn reset_state(&mut self) {
        for entry in &mut self.entries {
            if entry.status.bypassed {
                continue;
            }
            if let Some(processor) = &mut entry.processor {
                if let Err(error) = processor.reset() {
                    entry.status.bypassed = true;
                    entry.status.error = Some(format!("{error:#}"));
                }
            }
        }
    }
    pub fn statuses(&self) -> Vec<FilterStatus> {
        self.entries
            .iter()
            .map(|entry| entry.status.clone())
            .collect()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn has_stage(&self, stage: Stage) -> bool {
        self.entries.iter().any(|entry| {
            entry.config.enabled && entry.config.stage == stage && !entry.status.bypassed
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewSignal {
    #[default]
    Noise,
    Step,
    Sine,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewPoint {
    pub t: f64,
    pub input: [f64; 3],
    pub output: [f64; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewReport {
    pub points: Vec<PreviewPoint>,
    pub status: FilterStatus,
}

/// Runs a single filter against a deterministic 500 Hz synthetic trace.
/// No engine, provider, file-system or desktop-input service is involved.
pub fn preview(mut config: FilterConfig, signal: PreviewSignal) -> Result<PreviewReport> {
    config.enabled = true;
    let mut chain = FilterChain::new(&[config.clone()])?;
    let mut points = Vec::with_capacity(300);
    for index in 0..300 {
        let n = f64::from(index);
        let t = n * 0.002;
        let input = match signal {
            PreviewSignal::Noise => [
                (n * 2.37).sin() * 0.6 + (n * 4.17).sin() * 0.25,
                (n * 1.91).sin() * 0.4,
                (n * 2.71).cos() * 0.25,
            ],
            PreviewSignal::Step => [
                if index < 75 {
                    0.
                } else if index < 200 {
                    30.
                } else {
                    0.
                },
                if index < 150 { 0. } else { -15. },
                0.,
            ],
            PreviewSignal::Sine => [
                (t * std::f64::consts::TAU * 4.).sin() * 30.,
                (t * std::f64::consts::TAU * 2.).sin() * 15.,
                (t * std::f64::consts::TAU).sin() * 5.,
            ],
        };
        let input = if config.stage == Stage::RawGyro {
            input
        } else {
            [input[0], input[1], 0.]
        };
        let sample = Sample {
            value: input,
            raw: input,
            accel_g: Some([0., 0., -1.]),
            dt: 0.002,
            host_ns: 1_000_000 + index as u64 * 2_000_000,
            sensor_ns: 100_000_000 + index as u64 * 2_000_000,
        };
        let output = chain.process(config.stage, sample);
        points.push(PreviewPoint { t, input, output });
    }
    Ok(PreviewReport {
        points,
        status: chain
            .statuses()
            .pop()
            .context("Missing filter preview status")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(source: &str) -> FilterConfig {
        FilterConfig {
            id: "test".into(),
            name: "Test".into(),
            source: source.into(),
            ..Default::default()
        }
    }
    fn sample() -> Sample {
        Sample {
            value: [10., 20., 30.],
            raw: [1., 2., 3.],
            accel_g: None,
            dt: 0.002,
            host_ns: 1_234_567,
            sensor_ns: 9_876_543,
        }
    }
    const IDENTITY: &str = "fn process(s,p) { [s.x,s.y,s.z] }";
    #[test]
    fn identity_preserves_timed_samples_and_does_not_replay() {
        let mut chain=FilterChain::new(&[config("fn process(s,p) { if s.host_ns != 1234567 || s.sensor_ns != 9876543 {throw \"clock\";} [s.x,s.y,s.z] }")]).unwrap();
        assert_eq!(
            chain.process(Stage::BeforeBuiltin, sample()),
            [10., 20., 30.]
        );
        assert_eq!(chain.statuses()[0].calls, 1);
        assert_eq!(chain.process(Stage::RawGyro, sample()), [10., 20., 30.]);
        assert_eq!(chain.statuses()[0].calls, 1);
    }
    #[test]
    fn state_is_independent_and_resettable() {
        let mut c = config(
            "fn process(s,p) {if !this.contains(\"n\"){this.n=0.0;}this.n+=1.0;[this.n,s.y,s.z]}",
        );
        let mut chain = FilterChain::new(&[c.clone()]).unwrap();
        assert_eq!(chain.process(c.stage, sample())[0], 1.);
        assert_eq!(chain.process(c.stage, sample())[0], 2.);
        c.name = "Renamed".into();
        chain.reconfigure(&[c.clone()]).unwrap();
        assert_eq!(chain.process(c.stage, sample())[0], 3.);
        chain.reset();
        assert_eq!(chain.process(c.stage, sample())[0], 1.);
    }
    #[test]
    fn params_and_chain_order_are_applied() {
        let mut a = config("fn process(s,p) {[s.x+p.amount,s.y,s.z]}");
        a.params.insert("amount".into(), 2.);
        let mut b = config("fn process(s,p) {[s.x*3.0,s.y,s.z]}");
        b.id = "second".into();
        let mut chain = FilterChain::new(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(chain.process(a.stage, sample())[0], 36.);
        chain.reconfigure(&[b, a]).unwrap();
        assert_eq!(chain.process(Stage::BeforeBuiltin, sample())[0], 32.);
    }
    #[test]
    fn runtime_faults_and_invalid_results_are_sticky_bypasses() {
        for source in [
            "fn process(s,p){throw \"bad\";}",
            "fn process(s,p){[0.0/0.0,0.0,0.0]}",
            "fn process(s,p){[1,2]}",
            "fn process(s,p){[\"x\",2,3]}",
            "fn process(s,p){[1000001.0,2,3]}",
            "fn process(s,p){loop{}}",
        ] {
            let mut chain = FilterChain::new(&[config(source)]).unwrap();
            assert_eq!(
                chain.process(Stage::BeforeBuiltin, sample()),
                sample().value
            );
            let status = chain.statuses().remove(0);
            assert!(status.bypassed, "{source}");
            assert!(status.error.is_some());
            chain.process(Stage::BeforeBuiltin, sample());
            assert_eq!(chain.statuses()[0].calls, 1);
        }
    }
    #[test]
    fn no_modules_and_bounded_initialization() {
        assert!(FilterChain::new(&[config(
            "import \"secret\" as x; fn process(s,p){[s.x,s.y,s.z]}"
        )])
        .is_err());
        assert!(FilterChain::new(&[config("loop{} fn process(s,p){[s.x,s.y,s.z]}")]).is_err());
        assert!(FilterChain::new(&[config("fn other(s,p){}")]).is_err());
        let mut disabled = config("invalid code");
        disabled.enabled = false;
        assert!(FilterChain::new(&[disabled]).is_ok());
    }
    #[test]
    fn failed_reconfiguration_keeps_the_running_chain() {
        let mut chain = FilterChain::new(&[config(IDENTITY)]).unwrap();
        assert!(chain.reconfigure(&[config("invalid")]).is_err());
        assert_eq!(
            chain.process(Stage::BeforeBuiltin, sample()),
            sample().value
        );
    }
    #[test]
    fn bundled_templates_run_on_synthetic_data_and_smooth_noise() {
        for (source, params) in [
            (
                include_str!("../../../examples/filters/passthrough.rhai"),
                BTreeMap::new(),
            ),
            (
                include_str!("../../../examples/filters/ema.rhai"),
                BTreeMap::from([("tau_ms".into(), 12.)]),
            ),
            (
                include_str!("../../../examples/filters/median.rhai"),
                BTreeMap::from([("window".into(), 3.)]),
            ),
        ] {
            let mut c = config(source);
            c.params = params;
            let report = preview(c, PreviewSignal::Noise).unwrap();
            assert!(!report.status.bypassed, "{:?}", report.status.error);
            assert_eq!(report.points.len(), 300);
            assert_eq!(report.status.calls, 300);
            let input: f64 = report
                .points
                .iter()
                .map(|point| point.input[0] * point.input[0])
                .sum();
            let output: f64 = report
                .points
                .iter()
                .map(|point| point.output[0] * point.output[0])
                .sum();
            assert!(output <= input + 1e-6);
        }
    }
    #[test]
    fn automatic_history_reset_does_not_retry_a_faulted_filter() {
        let mut chain = FilterChain::new(&[config("fn process(s,p){throw \"fault\";}")]).unwrap();
        chain.process(Stage::BeforeBuiltin, sample());
        chain.reset_state();
        chain.process(Stage::BeforeBuiltin, sample());
        assert_eq!(chain.statuses()[0].calls, 1);
        assert!(chain.statuses()[0].bypassed);
        chain.reset();
        chain.process(Stage::BeforeBuiltin, sample());
        assert_eq!(chain.statuses()[0].calls, 1);
    }
    #[test]
    fn oversized_state_is_bypassed_and_disabled_filters_stay_idle() {
        let mut chain = FilterChain::new(&[config(
            "fn process(s,p){this.a=[];for n in 0..129{this.a.push(n);}[s.x,s.y,s.z]}",
        )])
        .unwrap();
        assert_eq!(
            chain.process(Stage::BeforeBuiltin, sample()),
            sample().value
        );
        assert!(chain.statuses()[0].bypassed);
        let mut c = config(IDENTITY);
        c.enabled = false;
        let mut chain = FilterChain::new(&[c]).unwrap();
        chain.process(Stage::BeforeBuiltin, sample());
        assert_eq!(chain.statuses()[0].calls, 0);
    }
    #[test]
    fn shared_ast_keeps_each_instances_state_private() {
        let source =
            "fn process(s,p){if !this.contains(\"n\"){this.n=0.0;}this.n+=1.0;[this.n,s.y,s.z]}";
        let a = config(source);
        let mut b = a.clone();
        b.id = "second".into();
        let mut chain = FilterChain::new(&[a, b]).unwrap();
        assert_eq!(chain.process(Stage::BeforeBuiltin, sample())[0], 1.);
        assert_eq!(chain.process(Stage::BeforeBuiltin, sample())[0], 2.);
    }
    #[test]
    fn operation_budget_is_shared_by_all_stages_in_a_sample() {
        let source = "fn process(s,p){let n=0;for i in 0..400{n+=i;}[s.x,s.y,s.z]}";
        let mut configs = vec![];
        for (index, stage) in [
            Stage::RawGyro,
            Stage::BeforeBuiltin,
            Stage::AfterBuiltin,
            Stage::AfterBuiltin,
        ]
        .into_iter()
        .enumerate()
        {
            let mut c = config(source);
            c.id = format!("filter{index}");
            c.stage = stage;
            configs.push(c);
        }
        let mut single = FilterChain::new(&configs[..1]).unwrap();
        single.process(Stage::RawGyro, sample());
        assert!(!single.statuses()[0].bypassed);
        let mut chain = FilterChain::new(&configs).unwrap();
        for stage in [Stage::RawGyro, Stage::BeforeBuiltin, Stage::AfterBuiltin] {
            chain.process(stage, sample());
        }
        assert!(chain.statuses().iter().any(|status| status.bypassed));
    }
    #[test]
    fn fifo_samples_with_identical_clocks_each_receive_their_own_budget() {
        let mut chain = FilterChain::new(&[config(
            "fn process(s,p){let n=0;for i in 0..400{n+=i;}[s.x,s.y,s.z]}",
        )])
        .unwrap();
        for _ in 0..10 {
            chain.begin_sample();
            assert_eq!(
                chain.process(Stage::BeforeBuiltin, sample()),
                sample().value
            );
        }
        assert_eq!(chain.statuses()[0].calls, 10);
        assert!(!chain.statuses()[0].bypassed);
    }
    #[test]
    fn nested_eval_is_disabled_and_print_is_redirected() {
        assert!(
            FilterChain::new(&[config("fn process(s,p){eval(\"1+1\");[s.x,s.y,s.z]}")]).is_err()
        );
        let report = preview(
            config("fn process(s,p){print(\"not worker stdout\");[s.x,s.y,s.z]}"),
            PreviewSignal::Step,
        )
        .unwrap();
        assert!(!report.status.bypassed);
    }
}
