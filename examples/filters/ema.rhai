// Time-based EMA. Seed from the first sample to avoid an initial ramp.
fn process(sample, params) {
    if params.tau_ms <= 0.0 { return [sample.x, sample.y, sample.z]; }
    if !this.contains("x") {
        this.x = sample.x;
        this.y = sample.y;
        this.z = sample.z;
    }
    let alpha = 1.0 - exp(-sample.dt / (params.tau_ms / 1000.0));
    this.x += alpha * (sample.x - this.x);
    this.y += alpha * (sample.y - this.y);
    this.z += alpha * (sample.z - this.z);
    [this.x, this.y, this.z]
}
