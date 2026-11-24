//! What [`Store`](crate::Store) keeps between tool calls (issue #71): open
//! read-only connections to `messages.db`, and the contact names. Before,
//! every tool call opened a connection and loaded every name again.
//!
//! What can change underneath, and how each is noticed:
//!
//! - **New messages and names:** the bridge writes all the time. SQLite's
//!   `PRAGMA data_version` says when another connection has written; names
//!   are then loaded again, at most every [`NAMES_MAX_AGE`].
//! - **The schema:** handled per connection by `Db::schema`.
//! - **The file itself:** deleted and created again (to link again), an open
//!   connection would keep reading the old one. Each checkout compares the
//!   file's identity and drops the pool when it changed.

use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use crate::{Db, Names, Result};

/// The contact names, shared by every connection's SQL functions, and
/// replaced as a whole when they are loaded again.
pub(crate) type SharedNames = Arc<RwLock<Arc<Names>>>;

/// How old the names may get before a write to the database loads them
/// again. Names change rarely; loading them reads every contact.
pub const NAMES_MAX_AGE: Duration = Duration::from_secs(30);

/// Idle connections kept for the next calls.
const MAX_IDLE: usize = 4;

/// A file's identity: device and inode. `None` where there is no such
/// thing; there an open file cannot be deleted anyway (Windows).
#[cfg(unix)]
fn file_identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn file_identity(_path: &Path) -> Option<(u64, u64)> {
    None
}

/// An idle connection and what it saw last.
struct Idle {
    db: Db,
    /// `PRAGMA data_version` when it was last used.
    data_version: i64,
    file: Option<(u64, u64)>,
}

#[derive(Default)]
pub(crate) struct Cache {
    idle: Mutex<Vec<Idle>>,
    names: SharedNames,
    age: Mutex<NamesAge>,
}

/// When the names were loaded, and whether the database was written since.
#[derive(Default)]
struct NamesAge {
    /// `None` before the first time.
    loaded: Option<Instant>,
    /// A write was seen while the names were still recent: remembered until
    /// they are loaded again, or it would be missed by the time they are old.
    written: bool,
}

impl Cache {
    /// A connection to `messages.db`, with names that are recent enough.
    pub(crate) fn checkout(
        self: &Arc<Self>,
        messages_db: &Path,
        whatsapp_db: &Path,
    ) -> Result<PooledDb> {
        let file = file_identity(messages_db);
        let reused = {
            let mut idle = self.idle.lock().unwrap_or_else(PoisonError::into_inner);
            if idle.last().is_some_and(|i| i.file != file) {
                // Another file under the same name: drop every connection.
                idle.clear();
            }
            idle.pop()
        };
        let (db, last_version) = match reused {
            Some(idle) => (idle.db, Some(idle.data_version)),
            None => (Db::open_shared(messages_db, Arc::clone(&self.names))?, None),
        };
        let version: i64 = db
            .conn
            .query_row("PRAGMA data_version", [], |row| row.get(0))?;
        // A new connection has no earlier version: count it as a change.
        let written = last_version != Some(version);
        self.refresh_names(messages_db, whatsapp_db, written);
        Ok(PooledDb {
            db: Some(db),
            data_version: version,
            file,
            cache: Arc::clone(self),
        })
    }

    /// Loads the names the first time, and again after a write once they
    /// are older than [`NAMES_MAX_AGE`].
    fn refresh_names(&self, messages_db: &Path, whatsapp_db: &Path, written: bool) {
        let mut age = self.age.lock().unwrap_or_else(PoisonError::into_inner);
        age.written |= written;
        let due = match age.loaded {
            None => true,
            Some(at) => age.written && at.elapsed() >= NAMES_MAX_AGE,
        };
        if !due {
            return;
        }
        let names = Arc::new(Names::load(messages_db, whatsapp_db));
        *self.names.write().unwrap_or_else(PoisonError::into_inner) = names;
        *age = NamesAge {
            loaded: Some(Instant::now()),
            written: false,
        };
    }

    /// Makes the names count as old (tests).
    pub(crate) fn expire_names(&self) {
        let mut age = self.age.lock().unwrap_or_else(PoisonError::into_inner);
        if age.loaded.is_some() {
            age.loaded = Instant::now().checked_sub(NAMES_MAX_AGE);
        }
    }

    fn give_back(&self, idle: Idle) {
        let mut pool = self.idle.lock().unwrap_or_else(PoisonError::into_inner);
        if pool.len() < MAX_IDLE {
            pool.push(idle);
        }
    }
}

/// A connection from the cache; it goes back when dropped. Use it as a
/// [`Db`].
pub struct PooledDb {
    db: Option<Db>,
    data_version: i64,
    file: Option<(u64, u64)>,
    cache: Arc<Cache>,
}

impl Deref for PooledDb {
    type Target = Db;

    fn deref(&self) -> &Db {
        self.db.as_ref().expect("taken only on drop")
    }
}

impl Drop for PooledDb {
    fn drop(&mut self) {
        if let Some(db) = self.db.take() {
            self.cache.give_back(Idle {
                db,
                data_version: self.data_version,
                file: self.file,
            });
        }
    }
}
