use std::sync::OnceLock;
use tokio::sync::{mpsc, Mutex as AsyncMutex};

/// A single background OS thread reads stdin and sends chars into this channel.
/// Using `std::thread::spawn` (not `spawn_blocking`) means tokio does not wait
/// for it on shutdown, so the process can exit cleanly without an extra keypress.
///
/// # Lifetime / known limitations
///
/// - The reader thread is started lazily on the first `getchar` call. Programs
///   that never call `getchar` pay no cost; programs that do leak the thread
///   for the rest of the process lifetime (it holds `stdin().lock()`).
/// - Once started, any other code in the same process that tries to read stdin
///   will deadlock. The CLI runs one program then exits, so this is fine, but
///   embedders that reuse a process across multiple program runs will see a
///   stale `Receiver` (the second run's `getchar` will consume bytes the first
///   run buffered, or block forever after EOF). Document this if it ever
///   matters; the fix is to move the receiver into per-`Interpreter` state.
pub(super) fn stdin_receiver() -> &'static AsyncMutex<mpsc::Receiver<Option<char>>> {
    static INSTANCE: OnceLock<AsyncMutex<mpsc::Receiver<Option<char>>>> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        let (tx, rx) = mpsc::channel(256);
        std::thread::spawn(move || {
            use std::io::Read;
            let stdin = std::io::stdin();
            let mut handle = stdin.lock();
            let mut first = [0u8; 1];
            loop {
                match handle.read(&mut first) {
                    Ok(0) | Err(_) => { let _ = tx.blocking_send(None); break; }
                    Ok(_) => {
                        let byte = first[0];
                        let seq_len = if byte < 0x80 { 1 }
                                      else if byte < 0xE0 { 2 }
                                      else if byte < 0xF0 { 3 }
                                      else { 4 };
                        let mut buf = [0u8; 4];
                        buf[0] = byte;
                        if seq_len > 1 && handle.read_exact(&mut buf[1..seq_len]).is_err() {
                            let _ = tx.blocking_send(None);
                            break;
                        }
                        let ch = std::str::from_utf8(&buf[..seq_len])
                            .ok()
                            .and_then(|s| s.chars().next());
                        if tx.blocking_send(ch).is_err() { break; }
                    }
                }
            }
        });
        AsyncMutex::new(rx)
    })
}
