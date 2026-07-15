use std::collections::HashMap;

use crate::usecase::{ErasedUseCase, UseCase};

/// name → UseCase のマップ。3 driver がここを引いて同じ能力セットを共有する。
/// MCP の tool 一覧・JSON Schema 自動導出は将来このレジストリに乗る（Phase 5-1）。
pub struct Registry {
    map: HashMap<&'static str, Box<dyn ErasedUseCase>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// 型安全な UseCase を登録する。ブランケット実装により Box<dyn ErasedUseCase> に消去される。
    pub fn register<T: UseCase + 'static>(&mut self, uc: T) {
        let name = UseCase::name(&uc);
        self.map.insert(name, Box::new(uc));
    }

    /// name で消去済み UseCase を引く。
    pub fn lookup(&self, name: &str) -> Option<&dyn ErasedUseCase> {
        self.map.get(name).map(|b| b.as_ref())
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::context::Ctx;
    use crate::error::AppError;
    use crate::usecase::{Risk, UseCase};

    #[derive(Deserialize)]
    struct EchoInput {
        text: String,
    }

    #[derive(Serialize)]
    struct EchoOutput {
        echoed: String,
    }

    struct EchoUseCase;

    #[async_trait::async_trait]
    impl UseCase for EchoUseCase {
        type Input = EchoInput;
        type Output = EchoOutput;

        fn name(&self) -> &'static str {
            "echo"
        }

        fn risk(&self, _input: &Self::Input) -> Risk {
            Risk::Read
        }

        async fn run(&self, input: Self::Input, _ctx: &Ctx) -> Result<Self::Output, AppError> {
            Ok(EchoOutput { echoed: input.text })
        }
    }

    #[test]
    fn test_register_and_lookup() {
        let mut reg = Registry::new();
        reg.register(EchoUseCase);

        let uc = reg.lookup("echo").expect("echo should be registered");
        assert_eq!(uc.name(), "echo");
    }

    #[test]
    fn test_lookup_unknown_returns_none() {
        let reg = Registry::new();
        assert!(reg.lookup("missing").is_none());
    }
}
