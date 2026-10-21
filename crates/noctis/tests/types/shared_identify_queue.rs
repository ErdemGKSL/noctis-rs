// `Shared::new(..)` is accepted only by the seven byte-store setters: the
// identify queue is coordination keyed by bucket, never namespaced (AD-14).
use noctis::ports::Shared;
use noctis::ports::memory::MemoryIdentifyQueue;

fn main() {
    let mut builder = noctis::App::builder();
    builder.identify_queue(Shared::new(MemoryIdentifyQueue::default()));
}
