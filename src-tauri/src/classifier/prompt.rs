use crate::models::classifier::{CorrectionEntry, MailSummary, ProjectSummary};

pub const SYSTEM_PROMPT: &str = "\
You are an email classifier. Given an email and a list of existing projects,
determine which project the email belongs to.

Respond with ONLY a JSON object in one of these formats:

1. Assign to existing project:
{\"action\": \"assign\", \"project_id\": \"<id>\", \"confidence\": 0.85, \"reason\": \"...\"}

2. Propose new project:
{\"action\": \"create\", \"project_name\": \"<name>\", \"description\": \"<desc>\", \"confidence\": 0.78, \"reason\": \"...\"}

3. Cannot classify:
{\"action\": \"unclassified\", \"confidence\": 0.30, \"reason\": \"...\"}

Rules:
- confidence is a float between 0.0 and 1.0
- reason is a brief explanation in Japanese
- When no existing project matches well, use \"create\" to propose a new one
- Use \"unclassified\" only when the email content is too ambiguous to classify";

pub fn build_user_prompt(
    mail: &MailSummary,
    projects: &[ProjectSummary],
    corrections: &[CorrectionEntry],
) -> String {
    let mut prompt = format!(
        "## Email to classify\n\
         Subject: {}\n\
         From: {}\n\
         Date: {}\n\
         Body preview: {}\n",
        mail.subject, mail.from_addr, mail.date, mail.body_preview
    );

    prompt.push_str("\n## Existing projects\n");
    if projects.is_empty() {
        prompt.push_str("(No existing projects)\n");
    } else {
        for project in projects {
            prompt.push_str(&format!(
                "- id: {}, name: {}{}\n",
                project.id,
                project.name,
                project
                    .description
                    .as_deref()
                    .map(|d| format!(", description: {}", d))
                    .unwrap_or_default()
            ));
            if !project.recent_subjects.is_empty() {
                prompt.push_str(&format!(
                    "  Recent subjects: {}\n",
                    project.recent_subjects.join("; ")
                ));
            }
        }
    }

    if !corrections.is_empty() {
        prompt.push_str("\n## Past corrections (user feedback)\n");
        for correction in corrections {
            let from = correction
                .from_project
                .as_deref()
                .unwrap_or("(unclassified)");
            prompt.push_str(&format!(
                "- \"{}\" was moved from {} to {}\n",
                correction.mail_subject, from, correction.to_project
            ));
        }
    }

    prompt.push_str("\nRespond with ONLY the JSON object.");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mail() -> MailSummary {
        MailSummary {
            subject: "Quarterly report review".to_string(),
            from_addr: "alice@example.com".to_string(),
            date: "2026-04-13".to_string(),
            body_preview: "Please review the attached quarterly report.".to_string(),
        }
    }

    fn make_project(id: &str, name: &str) -> ProjectSummary {
        ProjectSummary {
            id: id.to_string(),
            name: name.to_string(),
            description: Some(format!("Description for {}", name)),
            recent_subjects: vec!["Subject A".to_string(), "Subject B".to_string()],
        }
    }

    #[test]
    fn test_build_user_prompt_with_projects() {
        let mail = make_mail();
        let projects = vec![
            make_project("proj-1", "Finance"),
            make_project("proj-2", "Engineering"),
        ];
        let corrections = vec![];

        let prompt = build_user_prompt(&mail, &projects, &corrections);

        assert!(prompt.contains("Quarterly report review"));
        assert!(prompt.contains("alice@example.com"));
        assert!(prompt.contains("proj-1"));
        assert!(prompt.contains("Finance"));
        assert!(prompt.contains("proj-2"));
        assert!(prompt.contains("Engineering"));
        assert!(prompt.contains("Subject A"));
        assert!(!prompt.contains("Past corrections"));
        assert!(prompt.contains("Respond with ONLY the JSON object."));
    }

    #[test]
    fn test_build_user_prompt_no_projects() {
        let mail = make_mail();
        let projects = vec![];
        let corrections = vec![];

        let prompt = build_user_prompt(&mail, &projects, &corrections);

        assert!(prompt.contains("No existing projects"));
        assert!(!prompt.contains("Past corrections"));
    }

    #[test]
    fn test_build_user_prompt_with_corrections() {
        let mail = make_mail();
        let projects = vec![make_project("proj-1", "Finance")];
        let corrections = vec![
            CorrectionEntry {
                mail_subject: "Budget plan 2026".to_string(),
                from_project: Some("proj-2".to_string()),
                to_project: "proj-1".to_string(),
            },
            CorrectionEntry {
                mail_subject: "Kickoff meeting".to_string(),
                from_project: None,
                to_project: "proj-1".to_string(),
            },
        ];

        let prompt = build_user_prompt(&mail, &projects, &corrections);

        assert!(prompt.contains("Past corrections"));
        assert!(prompt.contains("Budget plan 2026"));
        assert!(prompt.contains("proj-2"));
        assert!(prompt.contains("proj-1"));
        assert!(prompt.contains("(unclassified)"));
        assert!(prompt.contains("Kickoff meeting"));
    }

    #[test]
    fn test_build_user_prompt_project_without_description() {
        let mail = make_mail();
        let projects = vec![ProjectSummary {
            id: "p1".to_string(),
            name: "No Desc Project".to_string(),
            description: None,
            recent_subjects: vec![],
        }];
        let prompt = build_user_prompt(&mail, &projects, &[]);
        assert!(prompt.contains("p1"));
        assert!(prompt.contains("No Desc Project"));
        assert!(!prompt.contains("description:"));
    }

    #[test]
    fn test_build_user_prompt_project_without_recent_subjects() {
        let mail = make_mail();
        let projects = vec![ProjectSummary {
            id: "p1".to_string(),
            name: "Empty Project".to_string(),
            description: Some("desc".to_string()),
            recent_subjects: vec![],
        }];
        let prompt = build_user_prompt(&mail, &projects, &[]);
        assert!(!prompt.contains("Recent subjects"));
    }

    #[test]
    fn test_build_user_prompt_many_corrections() {
        let mail = make_mail();
        let corrections: Vec<CorrectionEntry> = (0..5)
            .map(|i| CorrectionEntry {
                mail_subject: format!("Mail {}", i),
                from_project: Some(format!("proj-{}", i)),
                to_project: format!("proj-{}", i + 1),
            })
            .collect();
        let prompt = build_user_prompt(&mail, &[], &corrections);
        assert!(prompt.contains("Past corrections"));
        for i in 0..5 {
            assert!(prompt.contains(&format!("Mail {}", i)));
        }
    }

    #[test]
    fn test_build_user_prompt_contains_all_mail_fields() {
        let mail = MailSummary {
            subject: "特殊文字テスト <>&\"'".to_string(),
            from_addr: "日本語名前 <test@example.com>".to_string(),
            date: "2026-04-13".to_string(),
            body_preview: "本文プレビュー".to_string(),
        };
        let prompt = build_user_prompt(&mail, &[], &[]);
        assert!(prompt.contains("特殊文字テスト <>&\"'"));
        assert!(prompt.contains("日本語名前 <test@example.com>"));
        assert!(prompt.contains("本文プレビュー"));
    }
}
