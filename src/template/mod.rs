use std::collections::HashMap;

use minijinja::{
    Environment,
    UndefinedBehavior,
};

pub(crate) fn render_template(content: &str, vars: &HashMap<String, serde_json::Value>) -> crate::core::Result<String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    let result = env.render_str(content, vars)?;
    Ok(result)
}

pub(crate) fn merge_vars<'a, I>(vars: I) -> HashMap<String, serde_json::Value>
where
    I: IntoIterator<Item = &'a HashMap<String, serde_json::Value>>,
{
    let mut result = HashMap::new();

    for var_layer in vars {
        for (k, v) in var_layer {
            result.insert(k.to_string(), v.clone());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
            .collect()
    }

    #[test]
    fn variable_substitution_renders_correctly() {
        let result = render_template("Hello, {{ name }}!", &vars(&[("name", "world")])).unwrap();
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn local_vars_override_module_vars() {
        let module = vars(&[("theme", "default"), ("email", "global@example.com")]);
        let local = vars(&[("theme", "tokyonight")]);
        let merged = merge_vars([&module, &local]);
        assert_eq!(merged["theme"], serde_json::json!("tokyonight"));
        // email was not overridden
        assert_eq!(merged["email"], serde_json::json!("global@example.com"));
    }

    #[test]
    fn missing_variable_returns_error() {
        let result = render_template("Hello, {{ missing_var }}!", &HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn plain_content_without_vars_passes_through() {
        let content = "no template syntax here\njust text";
        let result = render_template(content, &HashMap::new()).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn angle_brackets_are_not_escaped() {
        // Confirm autoescape is off — config files must not have &lt; etc.
        let result = render_template("value = {{ val }}", &vars(&[("val", "<hello>")])).unwrap();
        assert_eq!(result, "value = <hello>");
    }

    #[test]
    fn later_layer_wins_on_conflict() {
        let a = vars(&[("x", "1")]);
        let b = vars(&[("x", "2")]);
        assert_eq!(merge_vars([&a, &b])["x"], serde_json::json!("2"));
    }

    #[test]
    fn keys_not_in_later_layer_are_preserved() {
        let a = vars(&[("x", "1"), ("y", "base")]);
        let b = vars(&[("x", "2")]);
        let merged = merge_vars([&a, &b]);
        assert_eq!(merged["x"], serde_json::json!("2"));
        assert_eq!(merged["y"], serde_json::json!("base"));
    }

    #[test]
    fn three_layer_resolution_rightmost_wins() {
        // global module vars < local config vars < local module vars
        let module = vars(&[("theme", "default"), ("font", "mono")]);
        let local = vars(&[("theme", "tokyonight")]);
        let per_mod = vars(&[("theme", "catppuccin")]);
        let merged = merge_vars([&module, &local, &per_mod]);
        assert_eq!(merged["theme"], serde_json::json!("catppuccin")); // per_mod wins                                                                                                                       
        assert_eq!(merged["font"], serde_json::json!("mono")); // falls through from module                                                                                                          
    }

    #[test]
    fn empty_layers_are_no_ops() {
        let a = vars(&[("x", "1")]);
        let empty = HashMap::new();
        assert_eq!(merge_vars([&empty, &a, &empty])["x"], serde_json::json!("1"));
    }
}
