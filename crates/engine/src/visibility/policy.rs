//! Driver-independent write-ahead ownership/rollback policy, exercised by mocks.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DriverState {
    pub active: bool,
    pub inverse: bool,
    pub hidden: Vec<String>,
    pub apps: Vec<String>,
}
pub trait Driver {
    fn read(&mut self) -> Result<DriverState>;
    fn set_hidden(&mut self, paths: &[String]) -> Result<()>;
    fn set_apps(&mut self, paths: &[String]) -> Result<()>;
    fn set_active(&mut self, active: bool) -> Result<()>;
}
pub trait Journal {
    fn load(&self) -> Result<Option<Record>>;
    fn save(&mut self, record: &Record) -> Result<()>;
    fn clear(&mut self) -> Result<()>;
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub version: u32,
    pub targets: Vec<String>,
    pub added: Vec<String>,
    pub engine: String,
    pub before_active: bool,
    pub before_hidden_digest: String,
    pub expected_apps_digest: String,
}
pub fn contains(list: &[String], value: &str) -> bool {
    list.iter().any(|item| item.eq_ignore_ascii_case(value))
}
pub fn push_unique(list: &mut Vec<String>, value: String) {
    if !contains(list, &value) {
        list.push(value);
    }
}
fn digest(list: &[String]) -> String {
    let mut canonical: Vec<_> = list.iter().map(|s| s.to_uppercase()).collect();
    canonical.sort();
    canonical.dedup();
    let mut hash = Sha256::new();
    for item in canonical {
        hash.update((item.len() as u64).to_le_bytes());
        hash.update(item.as_bytes());
    }
    format!("{:x}", hash.finalize())
}
pub fn virtual_id(id: &str) -> bool {
    let id = id.to_ascii_uppercase();
    // Ordinary USB hardware also reaches the ACPI computer root. ROOT is
    // otherwise conservative virtual ancestry, but this system node is not a
    // software controller. valid_physical_id still rejects it as a hide target.
    (id.starts_with("ROOT\\") && !id.starts_with("ROOT\\ACPI_HAL\\"))
        || id.starts_with("SWD\\")
        || id.contains("VIGEM")
        || id.contains("HIDMAESTRO")
        || id.starts_with("HID\\HIDCLASS\\")
}
pub fn valid_physical_id(id: &str) -> bool {
    let up = id.to_ascii_uppercase();
    id.len() <= 256
        && !id.contains(['\0', '\r', '\n'])
        && !virtual_id(id)
        && (up.starts_with("HID\\") || up.starts_with("USB\\VID_") || up.starts_with("BTHENUM\\"))
}
impl Record {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "未知的自动隐藏恢复记录版本");
        ensure!(
            !self.targets.is_empty() && self.targets.len() <= 8 && self.added.len() <= 8,
            "自动隐藏恢复记录不完整"
        );
        ensure!(
            self.targets.iter().all(|p| valid_physical_id(p))
                && self
                    .added
                    .iter()
                    .all(|p| valid_physical_id(p) && contains(&self.targets, p)),
            "恢复记录包含非实体手柄规则"
        );
        ensure!(
            self.engine.starts_with("\\Device\\")
                && self.engine.len() <= 32767
                && !self.engine.contains('\0'),
            "恢复记录的引擎路径无效"
        );
        for hash in [&self.before_hidden_digest, &self.expected_apps_digest] {
            ensure!(
                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                "自动隐藏恢复摘要损坏"
            );
        }
        Ok(())
    }
}

/// The recovery record is durable before the first driver write. Whitelist
/// registration is application authorization and intentionally persists.
pub fn acquire(
    driver: &mut impl Driver,
    journal: &mut impl Journal,
    targets: Vec<String>,
    engine: String,
) -> Result<Record> {
    ensure!(
        journal.load()?.is_none(),
        "上次自动隐藏尚未恢复，请先恢复隐藏规则"
    );
    let before = driver.read()?;
    ensure!(
        !before.inverse,
        "HidHide 正在使用反向应用列表，请先在官方工具改为普通模式"
    );
    let mut apps = before.apps.clone();
    push_unique(&mut apps, engine.clone());
    let added: Vec<_> = targets
        .iter()
        .filter(|p| !contains(&before.hidden, p))
        .cloned()
        .collect();
    let record = Record {
        version: 1,
        targets,
        added,
        engine,
        before_active: before.active,
        before_hidden_digest: digest(&before.hidden),
        expected_apps_digest: digest(&apps),
    };
    record.validate()?;
    journal
        .save(&record)
        .context("无法保存自动隐藏恢复记录，未修改隐藏规则")?;
    let result = (|| {
        if apps != before.apps {
            driver
                .set_apps(&apps)
                .context("HidHide 引擎读取权限设置失败")?;
        }
        let allowed = driver.read()?;
        ensure!(
            !allowed.inverse && contains(&allowed.apps, &record.engine),
            "HidHide 未确认引擎读取权限"
        );
        let mut hidden = allowed.hidden;
        for path in &record.targets {
            push_unique(&mut hidden, path.clone());
        }
        driver.set_hidden(&hidden).context("实体手柄隐藏失败")?;
        if !before.active {
            driver.set_active(true).context("启用 HidHide 失败")?;
        }
        check(driver, &record)
    })();
    if let Err(error) = result {
        return match restore(driver, journal) {
            Ok(()) => Err(error),
            Err(recovery) => Err(error.context(format!("隐藏未完成，恢复仍待处理：{recovery:#}"))),
        };
    }
    Ok(record)
}

pub fn check(driver: &mut impl Driver, record: &Record) -> Result<()> {
    let state = driver.read()?;
    ensure!(
        state.active && !state.inverse && contains(&state.apps, &record.engine),
        "HidHide 的启用状态或引擎权限已改变，映射已停止"
    );
    ensure!(
        record.targets.iter().all(|p| contains(&state.hidden, p)),
        "实体手柄隐藏规则已被更改，映射已停止"
    );
    Ok(())
}

pub fn restore(driver: &mut impl Driver, journal: &mut impl Journal) -> Result<()> {
    let Some(record) = journal.load()? else {
        return Ok(());
    };
    record.validate()?;
    let before = driver.read()?;
    let hidden: Vec<_> = before
        .hidden
        .iter()
        .filter(|p| !contains(&record.added, p))
        .cloned()
        .collect();
    if hidden != before.hidden {
        driver
            .set_hidden(&hidden)
            .context("恢复实体手柄可见性失败")?;
    }
    let after = driver.read()?;
    ensure!(
        !after.hidden.iter().any(|p| contains(&record.added, p)),
        "HidHide 尚未确认实体手柄恢复可见，恢复记录已保留"
    );
    // Do not turn off a global switch another application has started using.
    if !record.before_active
        && after.active
        && !after.inverse
        && digest(&after.hidden) == record.before_hidden_digest
        && digest(&after.apps) == record.expected_apps_digest
    {
        driver
            .set_active(false)
            .context("恢复 HidHide 原启用状态失败")?;
        ensure!(
            !driver.read()?.active,
            "HidHide 未确认原启用状态，恢复记录已保留"
        );
    }
    journal.clear()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;
    #[derive(Default)]
    struct Memory {
        record: Option<Record>,
        fail_save: bool,
    }
    impl Journal for Memory {
        fn load(&self) -> Result<Option<Record>> {
            Ok(self.record.clone())
        }
        fn save(&mut self, r: &Record) -> Result<()> {
            if self.fail_save {
                bail!("disk full")
            }
            self.record = Some(r.clone());
            Ok(())
        }
        fn clear(&mut self) -> Result<()> {
            self.record = None;
            Ok(())
        }
    }
    #[derive(Default)]
    struct Mock {
        state: DriverState,
        events: Vec<&'static str>,
        fail_hidden: bool,
        ignore_hidden: bool,
    }
    impl Driver for Mock {
        fn read(&mut self) -> Result<DriverState> {
            Ok(self.state.clone())
        }
        fn set_apps(&mut self, p: &[String]) -> Result<()> {
            self.events.push("allow");
            self.state.apps = p.to_vec();
            Ok(())
        }
        fn set_hidden(&mut self, p: &[String]) -> Result<()> {
            self.events.push("hide");
            if self.fail_hidden {
                bail!("write failed")
            }
            if !self.ignore_hidden {
                self.state.hidden = p.to_vec();
            }
            Ok(())
        }
        fn set_active(&mut self, on: bool) -> Result<()> {
            self.events.push(if on { "on" } else { "off" });
            self.state.active = on;
            Ok(())
        }
    }
    fn target() -> Vec<String> {
        vec![
            r"HID\VID_37D7&PID_2502\PHYSICAL".into(),
            r"USB\VID_37D7&PID_2502\RECEIVER".into(),
        ]
    }
    fn engine() -> String {
        r"\Device\HarddiskVolume3\Studio\motion-engine.exe".into()
    }
    #[test]
    fn whitelist_precedes_hiding_and_stop_restores_only_added_entries() {
        let mut d = Mock::default();
        d.state.hidden = vec!["external".into()];
        let mut j = Memory::default();
        acquire(&mut d, &mut j, target(), engine()).unwrap();
        assert_eq!(d.events, ["allow", "hide", "on"]);
        assert!(j.record.is_some());
        assert!(contains(&d.state.hidden, "external"));
        restore(&mut d, &mut j).unwrap();
        assert_eq!(d.state.hidden, ["external"]);
        assert!(!d.state.active);
        assert!(contains(&d.state.apps, &engine()));
        assert!(j.record.is_none());
    }
    #[test]
    fn preexisting_mask_and_active_state_survive_a_mapping_session() {
        let mut d = Mock::default();
        d.state.hidden = vec![target()[0].clone()];
        d.state.active = true;
        let before = d.state.hidden.clone();
        let mut j = Memory::default();
        acquire(&mut d, &mut j, target(), engine()).unwrap();
        restore(&mut d, &mut j).unwrap();
        assert_eq!(d.state.hidden, before);
        assert!(d.state.active);
    }
    #[test]
    fn outside_rule_changes_are_preserved_and_global_cloak_is_not_disabled() {
        let mut d = Mock::default();
        let mut j = Memory::default();
        acquire(&mut d, &mut j, target(), engine()).unwrap();
        d.state.hidden.push("other-new-rule".into());
        restore(&mut d, &mut j).unwrap();
        assert_eq!(d.state.hidden, ["other-new-rule"]);
        assert!(d.state.active);
    }
    #[test]
    fn failed_journal_save_cannot_change_driver_state() {
        let mut d = Mock::default();
        let mut j = Memory {
            fail_save: true,
            ..Default::default()
        };
        assert!(acquire(&mut d, &mut j, target(), engine()).is_err());
        assert!(d.events.is_empty());
    }
    #[test]
    fn partial_apply_failure_rolls_back_or_retains_a_recoverable_record() {
        let mut d = Mock {
            fail_hidden: true,
            ..Default::default()
        };
        let mut j = Memory::default();
        assert!(acquire(&mut d, &mut j, target(), engine()).is_err());
        assert!(!d.state.active);
        // A successful apply followed by a failed restore keeps the record.
        d.fail_hidden = false;
        acquire(&mut d, &mut j, target(), engine()).unwrap();
        d.fail_hidden = true;
        assert!(restore(&mut d, &mut j).is_err());
        assert!(j.record.is_some());
        d.fail_hidden = false;
        restore(&mut d, &mut j).unwrap();
        assert!(j.record.is_none());
    }
    #[test]
    fn ignored_writes_and_inverse_mode_never_report_success() {
        let mut d = Mock {
            ignore_hidden: true,
            ..Default::default()
        };
        let mut j = Memory::default();
        assert!(acquire(&mut d, &mut j, target(), engine()).is_err());
        d.state.inverse = true;
        d.events.clear();
        assert!(acquire(&mut d, &mut j, target(), engine()).is_err());
        assert!(d.events.is_empty());
    }
    #[test]
    fn virtual_ids_with_physical_vid_pid_cannot_enter_the_journal() {
        let mut d = Mock::default();
        let mut j = Memory::default();
        for id in [
            r"ROOT\ACPI_HAL\0000",
            r"ROOT\VID_054C&PID_09CC\001",
            r"SWD\HIDMAESTRO\MS01",
            r"HID\HIDCLASS\MS01",
            r"HID\VIGEM\VID_054C&PID_09CC",
        ] {
            assert!(acquire(&mut d, &mut j, vec![id.into()], engine()).is_err());
        }
        assert!(d.events.is_empty());
        assert!(j.record.is_none());
    }
    #[test]
    fn computer_root_is_not_virtual_but_cannot_be_hidden_as_a_controller() {
        for id in [r"ROOT\ACPI_HAL\0000", r"root\acpi_hal\0000"] {
            assert!(!virtual_id(id));
            assert!(!valid_physical_id(id));
        }
        for id in [
            r"ROOT\VIGEMBUS\0000",
            r"ROOT\HIDCLASS\MS01",
            r"ROOT\OTHER_BUS\0000",
            r"ROOT\ACPI_HAL_FAKE\0000",
            r"SWD\HIDMAESTRO\MS01",
        ] {
            assert!(virtual_id(id));
            assert!(!valid_physical_id(id));
        }
    }
    #[test]
    fn parent_crash_recovery_is_idempotent_and_does_not_need_the_device_present() {
        let mut d = Mock::default();
        let mut j = Memory::default();
        acquire(&mut d, &mut j, target(), engine()).unwrap();
        let mut recovered = Memory {
            record: Some(
                serde_json::from_slice(&serde_json::to_vec(j.record.as_ref().unwrap()).unwrap())
                    .unwrap(),
            ),
            ..Default::default()
        };
        restore(&mut d, &mut recovered).unwrap();
        restore(&mut d, &mut recovered).unwrap();
        assert!(d.state.hidden.is_empty());
        assert!(!d.state.active);
    }
    #[test]
    fn external_unhide_is_detected_without_reinstalling_the_rule() {
        let mut d = Mock::default();
        let mut j = Memory::default();
        let record = acquire(&mut d, &mut j, target(), engine()).unwrap();
        d.state.hidden.clear();
        let n = d.events.len();
        assert!(check(&mut d, &record).is_err());
        assert_eq!(d.events.len(), n);
        restore(&mut d, &mut j).unwrap();
    }
}
