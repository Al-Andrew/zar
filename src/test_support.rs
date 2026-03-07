use std::sync::{LazyLock, Mutex, MutexGuard};

static CWD_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

pub fn cwd_lock() -> MutexGuard<'static, ()> {
    CWD_LOCK.lock().expect("cwd lock")
}
