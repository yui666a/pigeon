pub mod account;
pub mod assign;
pub mod classify;
pub mod flag;
pub mod mailbox;
pub mod project;
pub mod search;
pub mod send;
pub mod sync;

use crate::usecase::Registry;

/// 全 use case をレジストリへ登録する（lib.rs から呼ぶ単一の入口）。
pub fn register_all(registry: &mut Registry) {
    account::register_account_cases(registry);
    search::register_read_cases(registry);
    flag::register_flag_cases(registry);
    mailbox::register_mailbox_cases(registry);
    assign::register_assign_cases(registry);
    send::register_send_cases(registry);
    project::register_project_cases(registry);
    sync::register_sync_cases(registry);
    classify::register_classify_cases(registry);
}
