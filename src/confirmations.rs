//! Remote endpoints that have matched our local ones *on their side*.
//!
//! DDS discovery is asymmetric: `RemoteWriterMatched` / `RemoteReaderMatched`
//! say that our side has matched a remote endpoint, but the remote side may
//! not have matched ours yet. Until it has, a request we send is dropped by
//! the remote Reader, and a reply the remote side sends never reaches us.
//! RustDDS reports when the remote side has matched too
//! (`RemoteWriterConfirmed` on a HEARTBEAT, `RemoteReaderConfirmed` on an
//! ACKNACK); the Spinner records those events here.

use std::{
  collections::{BTreeMap, BTreeSet},
  future::Future,
  sync::{Arc, Mutex},
  time::Duration,
};

use rustdds::{GUID, QosPolicies, policy};

/// Whether an endpoint with this QoS gets confirmed: only reliable ones do.
pub(crate) fn is_reliable(qos: &QosPolicies) -> bool {
  matches!(
    qos.reliability(),
    Some(policy::Reliability::Reliable { .. })
  )
}

#[derive(Clone, Default)]
pub(crate) struct Confirmations {
  // local endpoint -> remote endpoints that have matched it on their side
  confirmed: Arc<Mutex<BTreeMap<GUID, BTreeSet<GUID>>>>,
  waiters: Arc<Mutex<Vec<async_channel::Sender<()>>>>,
}

impl Confirmations {
  /// `remote` has matched `local` on its side.
  pub(crate) fn confirm(&self, local: GUID, remote: GUID) {
    self
      .confirmed
      .lock()
      .unwrap()
      .entry(local)
      .or_default()
      .insert(remote);
    // Wake every waiter; it re-checks its own endpoint. A full channel already
    // holds a wake-up, a closed one belongs to a finished waiter.
    self.waiters.lock().unwrap().retain(|waiter| {
      !matches!(
        waiter.try_send(()),
        Err(async_channel::TrySendError::Closed(_))
      )
    });
  }

  /// `remote` is gone: it no longer confirms anything.
  pub(crate) fn lost(&self, remote: GUID) {
    for remotes in self.confirmed.lock().unwrap().values_mut() {
      remotes.remove(&remote);
    }
  }

  /// Like [`wait`](Self::wait), but gives up after `timeout`: a confirmation
  /// event can be lost (a full status channel, or another Node's Spinner
  /// taking it). Returns whether `local` was confirmed.
  pub(crate) fn wait_at_most(
    &self,
    local: GUID,
    timeout: Duration,
  ) -> impl Future<Output = bool> + Send + 'static {
    let confirmed = self.wait(local);
    async move {
      futures::pin_mut!(confirmed);
      let give_up = futures_timer::Delay::new(timeout);
      matches!(
        futures::future::select(confirmed, give_up).await,
        futures::future::Either::Left(_)
      )
    }
  }

  /// Resolves once some remote endpoint has confirmed `local`.
  pub(crate) fn wait(&self, local: GUID) -> impl Future<Output = ()> + Send + 'static {
    // Register before the first check, so a confirmation between the two is
    // not missed.
    let (waker, woken) = async_channel::bounded(1);
    self.waiters.lock().unwrap().push(waker);
    let confirmed = Arc::clone(&self.confirmed);
    async move {
      loop {
        let done = confirmed
          .lock()
          .unwrap()
          .get(&local)
          .is_some_and(|remotes| !remotes.is_empty());
        if done || woken.recv().await.is_err() {
          return;
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use futures::FutureExt;

  use super::*;

  fn guid(n: u8) -> GUID {
    GUID::from_bytes([n; 16])
  }

  #[test]
  fn waiting_resolves_once_the_local_endpoint_is_confirmed() {
    let confirmations = Confirmations::default();
    let mut wait = Box::pin(confirmations.wait(guid(1)));
    assert!((&mut wait).now_or_never().is_none());

    confirmations.confirm(guid(2), guid(9)); // another local endpoint
    assert!((&mut wait).now_or_never().is_none());

    confirmations.confirm(guid(1), guid(9));
    assert!(wait.now_or_never().is_some());
  }

  #[test]
  fn an_already_confirmed_endpoint_resolves_at_once() {
    let confirmations = Confirmations::default();
    confirmations.confirm(guid(1), guid(9));
    assert!(confirmations.wait(guid(1)).now_or_never().is_some());
  }

  #[test]
  fn a_bounded_wait_reports_a_confirmation() {
    let confirmations = Confirmations::default();
    confirmations.confirm(guid(1), guid(9));
    let confirmed =
      futures::executor::block_on(confirmations.wait_at_most(guid(1), Duration::from_secs(5)));
    assert!(confirmed);
  }

  #[test]
  fn a_bounded_wait_gives_up_after_the_timeout() {
    let confirmations = Confirmations::default();
    let start = std::time::Instant::now();
    let confirmed =
      futures::executor::block_on(confirmations.wait_at_most(guid(1), Duration::from_millis(50)));
    assert!(!confirmed);
    assert!(start.elapsed() >= Duration::from_millis(50));
  }

  #[test]
  fn a_lost_remote_no_longer_confirms() {
    let confirmations = Confirmations::default();
    confirmations.confirm(guid(1), guid(9));
    confirmations.lost(guid(9));
    assert!(confirmations.wait(guid(1)).now_or_never().is_none());
  }
}
