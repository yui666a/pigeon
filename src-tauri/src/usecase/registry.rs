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

    /// 登録済み UseCase 名を昇順で返す。
    /// HashMap の反復順は非決定的なため、出力の安定のためにソートする。
    pub fn names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.map.keys().copied().collect();
        names.sort_unstable();
        names
    }

    /// 登録済み UseCase の名前と入力スキーマを昇順で返す。
    pub fn describe(&self) -> Vec<UseCaseInfo> {
        self.names()
            .into_iter()
            .filter_map(|name| {
                self.map.get(name).map(|uc| UseCaseInfo {
                    name,
                    input_schema: uc.input_schema(),
                })
            })
            .collect()
    }
}

/// レジストリに登録された 1 UseCase の外部公開情報。
/// MCP の tools/list と CLI の `call --list` が共用する。
#[derive(Debug, Clone, serde::Serialize)]
pub struct UseCaseInfo {
    pub name: &'static str,
    pub input_schema: serde_json::Value,
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

    #[derive(Deserialize, schemars::JsonSchema)]
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

        fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
            Ok(Risk::Read)
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

    #[test]
    fn test_names_is_sorted_and_contains_known_cases() {
        let mut reg = Registry::new();
        crate::usecase::cases::register_all(&mut reg);
        let names = reg.names();

        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "names() はソート済みで返す");

        assert!(names.contains(&"search_mails"));
        assert!(names.contains(&"mark_read"));
    }

    #[test]
    fn test_describe_returns_schema_with_properties() {
        let mut reg = Registry::new();
        crate::usecase::cases::register_all(&mut reg);
        let infos = reg.describe();

        let search = infos
            .iter()
            .find(|i| i.name == "search_mails")
            .expect("search_mails が登録されている");
        // SearchMailsInput の account_id / query が schema に現れる
        let props = &search.input_schema["properties"];
        assert!(
            props.get("account_id").is_some(),
            "schema: {}",
            search.input_schema
        );
        assert!(props.get("query").is_some());
    }
}
