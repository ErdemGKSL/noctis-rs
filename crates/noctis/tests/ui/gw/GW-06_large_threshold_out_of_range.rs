use noctis::gateway::LargeThreshold;

fn main() {
    let _ = LargeThreshold::new::<49>();
    let _ = LargeThreshold::new::<251>();
}
