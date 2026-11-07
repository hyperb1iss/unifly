use std::collections::HashMap;
use std::sync::{Arc, Weak};

use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::model::MacAddress;

/// Port overrides replace the whole device array. Serialize their read/modify/
/// write cycle per switch, without holding the shared session-client lock.
#[derive(Default)]
pub(super) struct PortWrites {
    active: Mutex<HashMap<MacAddress, Weak<Mutex<()>>>>,
}

impl PortWrites {
    pub(super) async fn acquire(&self, device: &MacAddress) -> OwnedMutexGuard<()> {
        let lock = {
            let mut active = self.active.lock().await;
            active.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = active.get(device).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(Mutex::new(()));
                active.insert(device.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        lock.lock_owned().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn only_writes_to_the_same_switch_contend() {
        let writes = PortWrites::default();
        let first = MacAddress::try_new("00:11:22:33:44:55").expect("MAC");
        let second = MacAddress::try_new("00:11:22:33:44:66").expect("MAC");
        let first_guard = writes.acquire(&first).await;
        let first_lock = writes
            .active
            .lock()
            .await
            .get(&first)
            .and_then(Weak::upgrade)
            .expect("active lock");
        assert!(first_lock.try_lock().is_err());
        let second_guard = writes.acquire(&second).await;
        drop(first_guard);
        drop(first_lock);
        let first_guard = writes.acquire(&first).await;
        assert_eq!(writes.active.lock().await.len(), 2);
        drop(first_guard);
        drop(second_guard);
        let _last = writes.acquire(&first).await;
        assert_eq!(writes.active.lock().await.len(), 1);
    }
}
