// Sliding median. Window is clamped to an odd number from 3 to 15.
fn process(sample, params) {
    if !this.contains("xs") {
        this.xs = [];
        this.ys = [];
        this.zs = [];
    }
    let count = params.window.to_int();
    if count < 3 { count = 3; }
    if count > 15 { count = 15; }
    if count % 2 == 0 { count -= 1; }
    this.xs.push(sample.x);
    this.ys.push(sample.y);
    this.zs.push(sample.z);
    if this.xs.len > count {
        this.xs.remove(0);
        this.ys.remove(0);
        this.zs.remove(0);
    }
    let xs = this.xs;
    let ys = this.ys;
    let zs = this.zs;
    xs.sort(); ys.sort(); zs.sort();
    let middle = xs.len / 2;
    [xs[middle], ys[middle], zs[middle]]
}
