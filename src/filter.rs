//! Command include/exclude/method filtering (bake mode).

use crate::model::CommandDef;

/// Filter by HTTP method whitelist, then include globs, then exclude globs.
///
/// MCP commands (`method` is None) pass the methods filter unchanged.
pub fn filter_commands(
    commands: Vec<CommandDef>,
    include: &[String],
    exclude: &[String],
    methods: &[String],
) -> Vec<CommandDef> {
    let mut result = commands;
    if !methods.is_empty() {
        let upper: Vec<String> = methods.iter().map(|m| m.to_uppercase()).collect();
        result.retain(|c| {
            c.method
                .as_ref()
                .map(|m| upper.iter().any(|u| u == &m.to_uppercase()))
                .unwrap_or(true)
        });
    }
    if !include.is_empty() {
        result.retain(|c| include.iter().any(|pat| glob_match(pat, &c.name)));
    }
    if !exclude.is_empty() {
        result.retain(|c| !exclude.iter().any(|pat| glob_match(pat, &c.name)));
    }
    result
}

/// Minimal glob: `*` matches any substring (fnmatch-style for bake patterns).
fn glob_match(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == name;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let mut rest = name;
    if !parts[0].is_empty() {
        if !rest.starts_with(parts[0]) {
            return false;
        }
        rest = &rest[parts[0].len()..];
    }
    for (i, part) in parts.iter().enumerate().skip(1) {
        if part.is_empty() {
            if i == parts.len() - 1 {
                return true;
            }
            continue;
        }
        if i == parts.len() - 1 {
            return rest.ends_with(part);
        }
        if let Some(idx) = rest.find(part) {
            rest = &rest[idx + part.len()..];
        } else {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds() -> Vec<CommandDef> {
        vec![
            CommandDef {
                name: "list-pets".into(),
                method: Some("GET".into()),
                ..Default::default()
            },
            CommandDef {
                name: "create-pet".into(),
                method: Some("POST".into()),
                ..Default::default()
            },
            CommandDef {
                name: "delete-pet".into(),
                method: Some("DELETE".into()),
                ..Default::default()
            },
            CommandDef {
                name: "update-pet".into(),
                method: Some("PUT".into()),
                ..Default::default()
            },
            CommandDef {
                name: "echo".into(),
                tool_name: Some("echo".into()),
                ..Default::default()
            },
        ]
    }

    fn names(cmds: &[CommandDef]) -> Vec<&str> {
        cmds.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn glob_basics() {
        assert!(glob_match("list-*", "list-pets"));
        assert!(!glob_match("list-*", "create-pet"));
        assert!(glob_match("*-pet", "get-pet"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn no_filters() {
        let c = cmds();
        assert_eq!(
            names(&filter_commands(c.clone(), &[], &[], &[])),
            names(&c)
        );
    }

    #[test]
    fn methods_filter() {
        let methods = vec!["GET".into(), "POST".into()];
        let result = filter_commands(cmds(), &[], &[], &methods);
        let n = names(&result);
        assert!(n.contains(&"list-pets"));
        assert!(n.contains(&"create-pet"));
        assert!(!n.contains(&"delete-pet"));
        assert!(!n.contains(&"update-pet"));
        assert!(n.contains(&"echo")); // MCP passes through
    }

    #[test]
    fn include_filter() {
        let include = vec!["list-*".into()];
        let result = filter_commands(cmds(), &include, &[], &[]);
        assert_eq!(names(&result), vec!["list-pets"]);
    }

    #[test]
    fn exclude_filter() {
        let exclude = vec!["delete-*".into(), "update-*".into()];
        let result = filter_commands(cmds(), &[], &exclude, &[]);
        let n = names(&result);
        assert!(n.contains(&"list-pets"));
        assert!(n.contains(&"create-pet"));
        assert!(!n.contains(&"delete-pet"));
        assert!(!n.contains(&"update-pet"));
    }

    #[test]
    fn combined_filters() {
        let methods = vec!["GET".into(), "POST".into()];
        let exclude = vec!["create-*".into()];
        let result = filter_commands(cmds(), &[], &exclude, &methods);
        assert_eq!(names(&result), vec!["list-pets", "echo"]);
    }

    #[test]
    fn include_and_exclude() {
        let include = vec!["*-pet".into()];
        let exclude = vec!["delete-*".into()];
        let result = filter_commands(cmds(), &include, &exclude, &[]);
        let n = names(&result);
        assert!(n.contains(&"create-pet"));
        assert!(n.contains(&"update-pet"));
        assert!(!n.contains(&"delete-pet"));
        assert!(!n.contains(&"list-pets"));
    }

    #[test]
    fn case_insensitive_methods() {
        let methods = vec!["get".into()];
        let result = filter_commands(cmds(), &[], &[], &methods);
        assert!(names(&result).contains(&"list-pets"));
    }
}
