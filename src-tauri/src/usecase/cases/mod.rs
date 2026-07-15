pub mod assign;
pub mod flag;
pub mod mailbox;
pub mod search;
pub mod send;

use crate::usecase::Registry;

/// 全 use case をレジストリへ登録する（lib.rs から呼ぶ単一の入口）。
pub fn register_all(registry: &mut Registry) {
    search::register_read_cases(registry);
    flag::register_flag_cases(registry);
    mailbox::register_mailbox_cases(registry);
    assign::register_assign_cases(registry);
    send::register_send_cases(registry);
}
