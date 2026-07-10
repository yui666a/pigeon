# GCP API 有効化インベントリ（Terraform 管理用メモ）

- 記録日: 2026-07-11
- プロジェクト: `<PROJECT_ID>`（具体的なプロジェクト ID はリポジトリに書かない方針。ローカルの settings / 環境変数でのみ保持し、Terraform 化時は変数 `var.project_id` を使う）
- 目的: Agent Platform（旧 Vertex AI）で Claude on Vertex を使うための API 有効化状況を記録し、後で Terraform（`google_project_service`）で宣言的に管理できるようにする。

## 1. Agent Platform 一括有効化で追加された API（2026-07-11 実測・確定）

コンソールの Agent Platform「必要な API の有効化」を押した結果、`gcloud services list --enabled` の**前後差分**で確定した新規有効化 API（19個）。これらが Terraform で `agent_platform_apis` として管理すべき対象。

| API サービス名（確定） | 用途の概略 |
|---|---|
| `agentregistry.googleapis.com` | Agent Registry |
| `apphub.googleapis.com` | App Hub |
| `apptopology.googleapis.com` | App Topology |
| `cloudapiregistry.googleapis.com` | Cloud API Registry |
| `compute.googleapis.com` | Compute Engine |
| `edgecache.googleapis.com` | Media CDN / Edge Cache |
| `iamconnectorcredentials.googleapis.com` | IAM Connectors 認証 |
| `iamconnectors.googleapis.com` | IAM Connectors |
| `iap.googleapis.com` | Cloud Identity-Aware Proxy |
| `modelarmor.googleapis.com` | Model Armor |
| `networksecurity.googleapis.com` | Network Security |
| `networkservices.googleapis.com` | Network Services |
| `notebooks.googleapis.com` | Notebooks |
| `observability.googleapis.com` | Observability |
| `oslogin.googleapis.com` | OS Login |
| `saasservicemgmt.googleapis.com` | SaaS Service Management |
| `securitycenter.googleapis.com` | Security Command Center |
| `securitycentermanagement.googleapis.com` | Security Command Center Management |
| `texttospeech.googleapis.com` | Cloud Text-to-Speech |

> これらは Agent Platform の**全機能**向け一括有効化。Pigeon の Claude/Gemini on Vertex（rawPredict / generateContent）には**大半が不要**（§2）。ただしユーザーの意向で有効化済みのため記録する。

## 2. Pigeon の Claude on Vertex に「実際に必須」な API

上記23個は Agent Platform の**全機能**を使うための一括推奨であり、Pigeon が rawPredict で Claude を叩くだけなら以下で十分:

- `aiplatform.googleapis.com` — Vertex AI / Agent Platform 本体（必須）
- `iam.googleapis.com` — サービスアカウント管理（SA 発行済み。必須）
- `iamcredentials.googleapis.com` — トークン生成（必須）
- `serviceusage.googleapis.com` — （既定で有効）

残りの Compute Engine / Notebooks / Text-to-Speech / Model Armor / Security Command Center 等は Pigeon の用途には**不要**。Terraform 化時は「必須セット」と「Agent Platform フル機能セット」を分けて管理するのが望ましい。

## 3. 識別子の確定方法（実施済み）

「有効にする」押下後、前後スナップショットの差分で §1 の識別子を確定済み（2026-07-11）:

```
gcloud services list --enabled --project="$PROJECT_ID" --format="value(config.name)" | sort > after.txt
comm -13 before.txt after.txt   # ＝ §1 の19個
```

## 4. 有効化「前」のスナップショット（2026-07-11 取得）

Claude on Vertex セットアップ中に有効化済みのものを含む、有効化ダイアログを押す**直前**の状態:

```
aiplatform.googleapis.com
analyticshub.googleapis.com
bigquery.googleapis.com
bigqueryconnection.googleapis.com
bigquerydatapolicy.googleapis.com
bigquerydatatransfer.googleapis.com
bigquerymigration.googleapis.com
bigqueryreservation.googleapis.com
bigquerystorage.googleapis.com
cloudapis.googleapis.com
cloudtrace.googleapis.com
dataform.googleapis.com
dataplex.googleapis.com
datastore.googleapis.com
iam.googleapis.com
iamcredentials.googleapis.com
logging.googleapis.com
monitoring.googleapis.com
servicemanagement.googleapis.com
serviceusage.googleapis.com
sql-component.googleapis.com
storage-api.googleapis.com
storage-component.googleapis.com
storage.googleapis.com
telemetry.googleapis.com
```

このうち Claude on Vertex セットアップで**私が明示的に有効化した**のは:
- `aiplatform.googleapis.com`
- `iam.googleapis.com`
- `iamcredentials.googleapis.com`

残りは、プロジェクト作成時のデフォルト有効化 API（BigQuery / Storage / Dataform / Datastore / Logging / Monitoring / Trace 等）。

## 5. Terraform 雛形（識別子確定済み）

```hcl
locals {
  # Pigeon の Claude/Gemini on Vertex に必須（最小セット）
  required_apis = [
    "aiplatform.googleapis.com",
    "iam.googleapis.com",
    "iamcredentials.googleapis.com",
    "serviceusage.googleapis.com",
  ]

  # 2026-07-11 に Agent Platform 一括有効化で追加された全機能向け API（§1 で確定）。
  # Pigeon の用途には大半が不要だが、現状に合わせて管理下に置く場合はこちらも apply する。
  agent_platform_apis = [
    "agentregistry.googleapis.com",
    "apphub.googleapis.com",
    "apptopology.googleapis.com",
    "cloudapiregistry.googleapis.com",
    "compute.googleapis.com",
    "edgecache.googleapis.com",
    "iamconnectorcredentials.googleapis.com",
    "iamconnectors.googleapis.com",
    "iap.googleapis.com",
    "modelarmor.googleapis.com",
    "networksecurity.googleapis.com",
    "networkservices.googleapis.com",
    "notebooks.googleapis.com",
    "observability.googleapis.com",
    "oslogin.googleapis.com",
    "saasservicemgmt.googleapis.com",
    "securitycenter.googleapis.com",
    "securitycentermanagement.googleapis.com",
    "texttospeech.googleapis.com",
  ]
}

resource "google_project_service" "required" {
  for_each = toset(local.required_apis)
  project  = var.project_id
  service  = each.value

  disable_on_destroy = false
}

# フル機能まで宣言的管理下に置く場合のみ有効化する。
resource "google_project_service" "agent_platform" {
  for_each = toset(local.agent_platform_apis)
  project  = var.project_id
  service  = each.value

  disable_on_destroy = false
}
```

> 注意: `disable_on_destroy = false` を推奨。`terraform destroy` で他リソースが依存する API まで無効化されるのを防ぐ。
