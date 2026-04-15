use rusqlite::Connection;
use std::sync::Mutex;

use crate::secure_store::SecureStore;

pub struct DbState(pub Mutex<Connection>);
pub struct SecureStoreState(pub SecureStore);
