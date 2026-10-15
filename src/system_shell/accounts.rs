use std::{collections::HashSet, fs};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LoginUser {
    pub name: String,
    pub label: String,
    pub uid: u32,
}

pub(crate) fn load() -> Result<Vec<LoginUser>, String> {
    let configured = fs::read_to_string("/etc/telorgon/login-users")
        .map_err(|error| format!("Could not read login user policy: {error}"))?;
    let passwd = fs::read_to_string("/etc/passwd")
        .map_err(|error| format!("Could not read local users: {error}"))?;
    parse(&configured, &passwd)
}

// The renderer reads only explicitly configured public account records. It never
// reads shadow passwords, a user's home directory or personal settings.
fn parse(configured: &str, passwd: &str) -> Result<Vec<LoginUser>, String> {
    let mut users = Vec::new();
    let mut names = HashSet::new();
    for name in configured.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        if !names.insert(name) {
            return Err(format!("Duplicate login user: {name}"));
        }
        let records: Vec<_> = passwd.lines().filter_map(|line| {
            let fields: Vec<_> = line.split(':').collect();
            (fields.first() == Some(&name)).then_some(fields)
        }).collect();
        let [fields] = records.as_slice() else {
            return Err(format!("Login user must have exactly one local account record: {name}"));
        };
        if fields.len() != 7 || !fields[2].parse::<u32>().is_ok_and(|uid| uid != 0)
            || matches!(fields[6].rsplit('/').next(), Some("" | "false" | "nologin")) {
            return Err(format!("Login user is not an interactive non-root account: {name}"));
        }
        let label = fields[4].split(',').next().unwrap_or("").trim();
        users.push(LoginUser {
            name: name.into(),
            label: if label.is_empty() { name } else { label }.into(),
            uid: fields[2].parse().unwrap(),
        });
    }
    Ok(users)
}

#[cfg(test)]
mod tests {
    use super::*;
    const PASSWD: &str = "root:x:0:0:Root:/root:/bin/sh\ncustom:x:1000:1000:Custom Distro user,room:/home/custom:/bin/sh\nother:x:1001:1001::/home/other:/bin/sh\nshell:x:979:979:Shell:/var/lib/shell:/bin/false\n";

    #[test]
    fn only_configured_users_are_shown_in_policy_order() {
        let users = parse("# Test accounts\nother\n\n custom \n", PASSWD).unwrap();
        assert_eq!(users.iter().map(|user| user.name.as_str()).collect::<Vec<_>>(), ["other", "custom"]);
        assert_eq!(users[0].label, "other");
        assert_eq!(users[1].label, "Custom Distro user");
    }

    #[test]
    fn root_and_noninteractive_service_accounts_are_rejected() {
        for name in ["root", "shell"] {
            assert!(parse(name, PASSWD).is_err());
        }
        assert!(parse("custom", &PASSWD.replace("custom:x:1000", "custom:x:0")).is_err());
        assert!(parse("custom", &PASSWD.replace("/home/custom:/bin/sh", "/home/custom:/usr/sbin/nologin")).is_err());
    }

    #[test]
    fn missing_duplicate_and_malformed_accounts_are_rejected() {
        assert!(parse("missing", PASSWD).is_err());
        assert!(parse("custom\ncustom", PASSWD).is_err());
        assert!(parse("custom", &format!("{PASSWD}custom:x:1002:1002:Alias:/home/alias:/bin/sh\n")).is_err());
        assert!(parse("custom", "custom:x:not-a-uid:1000:Custom:/home/custom:/bin/sh").is_err());
        assert!(parse("custom", "custom:x:1000").is_err());
    }

    #[test]
    fn no_configured_users_does_not_expose_other_accounts() {
        assert_eq!(parse("# No login users\n", PASSWD).unwrap(), []);
    }
}
