//! Keyring writes a setup wizard holds back until its last prompt.

use secrecy::{ExposeSecret, SecretString};

use crate::cli::error::CliError;
use crate::config::{SecretStore, SecretStoreError};

/// Secrets a wizard stores in the keyring only once every prompt is
/// answered, and takes back out if the config save after them fails, so an
/// abandoned or failed setup leaves the keyring as it found it. The CLI's
/// counterpart to the TUI's save plan.
#[derive(Default)]
pub(super) struct PendingSecrets {
    entries: Vec<PendingSecret>,
}

struct PendingSecret {
    key: String,
    label: &'static str,
    secret: SecretString,
}

/// A keyring entry a commit has replaced, and what it held before.
struct Written {
    key: String,
    label: &'static str,
    previous: Option<SecretString>,
}

impl PendingSecrets {
    /// Hold `secret` for the keyring entry `key`; nothing is stored yet.
    pub(super) fn add(&mut self, key: String, label: &'static str, secret: SecretString) {
        self.entries.push(PendingSecret { key, label, secret });
    }

    /// Store every held secret in `store`, then run `save`. Returns the
    /// labels of the secrets stored.
    ///
    /// If a secret is refused, or `save` fails, every entry this commit
    /// wrote is put back as it was (restored, or deleted when it was new)
    /// and the error is returned; an entry that could not be put back is
    /// named in it.
    pub(super) fn commit(
        self,
        store: &dyn SecretStore,
        save: impl FnOnce() -> Result<(), CliError>,
    ) -> Result<Vec<&'static str>, CliError> {
        let mut written = Vec::new();
        for pending in self.entries {
            let stored = store.get(&pending.key).and_then(|previous| {
                store.set(&pending.key, pending.secret.expose_secret())?;
                Ok(previous)
            });
            match stored {
                Ok(previous) => written.push(Written {
                    key: pending.key,
                    label: pending.label,
                    previous: previous.map(SecretString::from),
                }),
                Err(error) => {
                    return Err(roll_back(
                        keyring_error(pending.label, &error),
                        store,
                        &written,
                    ));
                }
            }
        }
        match save() {
            Ok(()) => Ok(written.iter().map(|entry| entry.label).collect()),
            Err(error) => Err(roll_back(error, store, &written)),
        }
    }
}

/// Put back the entries a failed commit wrote, newest first, and return
/// `error`, extended with any entry that could not be put back.
fn roll_back(error: CliError, store: &dyn SecretStore, written: &[Written]) -> CliError {
    let unrestored: Vec<String> = written
        .iter()
        .rev()
        .filter_map(|entry| {
            let restored = match &entry.previous {
                Some(previous) => store.set(&entry.key, previous.expose_secret()),
                None => store.delete(&entry.key),
            };
            restored.err().map(|failure| {
                format!(
                    "the {} it had already stored could not be put back ({failure})",
                    entry.label
                )
            })
        })
        .collect();
    if unrestored.is_empty() {
        error
    } else {
        CliError::Validation {
            field: "keyring".into(),
            reason: format!("{error}, and {}", unrestored.join(", and ")),
        }
    }
}

/// The error for a secret the keyring would not take. The CLI never falls
/// back to plaintext on its own: it says why, and plaintext storage stays
/// an explicit choice.
pub(super) fn keyring_error(label: &str, error: &SecretStoreError) -> CliError {
    CliError::Validation {
        field: "keyring".into(),
        reason: match error {
            SecretStoreError::Unavailable(_) => format!(
                "failed to store {label}, {error}; keep it in config.toml or an \
                 environment variable instead"
            ),
            SecretStoreError::Rejected(_) => format!("failed to store {label}, {error}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    use secrecy::SecretString;

    use super::PendingSecrets;
    use crate::cli::error::CliError;
    use crate::config::{SecretStore, SecretStoreError};

    /// In-memory stand-in for the OS keyring; `refuse` names a key it will
    /// not take.
    #[derive(Default)]
    struct MemoryStore {
        entries: RefCell<HashMap<String, String>>,
        refuse: Option<&'static str>,
    }

    impl MemoryStore {
        fn with(entries: &[(&str, &str)]) -> Self {
            let store = Self::default();
            for (key, secret) in entries {
                store
                    .entries
                    .borrow_mut()
                    .insert((*key).to_owned(), (*secret).to_owned());
            }
            store
        }

        fn secret(&self, key: &str) -> Option<String> {
            self.entries.borrow().get(key).cloned()
        }
    }

    impl SecretStore for MemoryStore {
        fn get(&self, key: &str) -> Result<Option<String>, SecretStoreError> {
            Ok(self.secret(key))
        }

        fn set(&self, key: &str, secret: &str) -> Result<(), SecretStoreError> {
            if self.refuse == Some(key) {
                return Err(SecretStoreError::Rejected("refused".into()));
            }
            self.entries
                .borrow_mut()
                .insert(key.to_owned(), secret.to_owned());
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), SecretStoreError> {
            self.entries.borrow_mut().remove(key);
            Ok(())
        }
    }

    fn pending() -> PendingSecrets {
        let mut secrets = PendingSecrets::default();
        secrets.add(
            "work/api-key".into(),
            "API key",
            SecretString::from("new-key"),
        );
        secrets.add(
            "work/password".into(),
            "Password",
            SecretString::from("new-password"),
        );
        secrets
    }

    fn failed_save() -> Result<(), CliError> {
        Err(CliError::Validation {
            field: "config".into(),
            reason: "disk full".into(),
        })
    }

    /// A wizard abandoned before its last prompt never committed, so the
    /// keyring is as it was.
    #[test]
    fn nothing_reaches_the_keyring_before_commit() {
        let store = MemoryStore::with(&[("work/api-key", "old-key")]);

        drop(pending());

        assert_eq!(store.secret("work/api-key").as_deref(), Some("old-key"));
        assert_eq!(store.secret("work/password"), None);
    }

    /// Replacing a profile and failing to save the config must not leave
    /// the old config pointing at a keyring entry that now holds the new
    /// secret.
    #[test]
    fn a_failed_save_puts_the_keyring_back() {
        let store = MemoryStore::with(&[("work/api-key", "old-key")]);

        let result = pending().commit(&store, failed_save);

        assert!(result.is_err());
        assert_eq!(store.secret("work/api-key").as_deref(), Some("old-key"));
        assert_eq!(store.secret("work/password"), None);
    }

    #[test]
    fn a_refused_secret_undoes_the_earlier_ones_and_skips_the_save() {
        let store = MemoryStore {
            refuse: Some("work/password"),
            ..MemoryStore::with(&[("work/api-key", "old-key")])
        };
        let saved = Cell::new(false);

        let result = pending().commit(&store, || {
            saved.set(true);
            Ok(())
        });

        assert!(result.is_err());
        assert!(!saved.get(), "the config was saved after a refused secret");
        assert_eq!(store.secret("work/api-key").as_deref(), Some("old-key"));
    }

    #[test]
    fn commit_stores_every_secret_then_saves() {
        let store = MemoryStore::default();

        let stored = pending().commit(&store, || Ok(())).expect("commit");

        assert_eq!(stored, ["API key", "Password"]);
        assert_eq!(store.secret("work/api-key").as_deref(), Some("new-key"));
        assert_eq!(
            store.secret("work/password").as_deref(),
            Some("new-password")
        );
    }
}
