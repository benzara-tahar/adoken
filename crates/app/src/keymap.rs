//! User bindings from `keymap.toml`, read off-thread after default bindings.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{Action, App, DummyKeyboardMapper, KeyBinding, KeyBindingContextPredicate};
use serde::Deserialize;

#[derive(Deserialize)]
struct Document {
    #[serde(default)]
    binding: Vec<Binding>,
}

#[derive(Deserialize)]
struct Binding {
    context: Option<String>,
    keys: String,
    action: String,
    /// Select a particular default binding for an action with several payloads.
    from: Option<String>,
    params: Option<toml::Value>,
}

fn predicate(context: Option<&str>) -> Result<Option<Rc<KeyBindingContextPredicate>>, String> {
    context
        .map(|context| {
            KeyBindingContextPredicate::parse(context)
                .map(Rc::new)
                .map_err(|error| error.to_string())
        })
        .transpose()
}

/// `keymap.toml` over `defaults` (also the cheat sheet's rebind test).
pub(crate) fn compile(
    text: &str,
    defaults: Vec<KeyBinding>,
    mut build: impl FnMut(&str, Option<serde_json::Value>) -> Result<Box<dyn Action>, String>,
) -> Result<Vec<KeyBinding>, String> {
    let document: Document = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut bindings = defaults.clone();
    for (index, entry) in document.binding.into_iter().enumerate() {
        let result = (|| {
            if entry.keys.trim().is_empty() {
                return Err("keys must contain a keystroke".into());
            }
            let context = predicate(entry.context.as_deref())?;
            let candidates: Vec<_> = defaults
                .iter()
                .filter(|binding| {
                    binding.action().name() == entry.action && binding.predicate() == context
                })
                .collect();
            let action = if let Some(from) = &entry.from {
                if entry.params.is_some() {
                    return Err("from and params cannot be combined".into());
                }
                let probe = KeyBinding::load(
                    from,
                    Box::new(gpui::NoAction),
                    context.clone(),
                    false,
                    None,
                    &DummyKeyboardMapper,
                )
                .map_err(|error| error.to_string())?;
                candidates
                    .iter()
                    .find(|binding| binding.keystrokes() == probe.keystrokes())
                    .ok_or_else(|| format!("no default binding for {} at {from}", entry.action))?
                    .action()
                    .boxed_clone()
            } else if let Some(params) = entry.params {
                let params = serde_json::to_value(params).map_err(|error| error.to_string())?;
                build(&entry.action, Some(params))?
            } else if let Some(first) = candidates.first() {
                if candidates
                    .iter()
                    .any(|candidate| !candidate.action().partial_eq(first.action()))
                {
                    return Err(format!(
                        "{} has several targets; select one with from or params",
                        entry.action
                    ));
                }
                first.action().boxed_clone()
            } else {
                build(&entry.action, None)?
            };
            let replacement = KeyBinding::load(
                &entry.keys,
                action,
                context.clone(),
                false,
                None,
                &DummyKeyboardMapper,
            )
            .map_err(|error| error.to_string())?;
            bindings.retain(|binding| {
                !(binding.predicate() == context
                    && binding.action().partial_eq(replacement.action()))
            });
            bindings.push(replacement);
            Ok(())
        })();
        result.map_err(|error: String| format!("binding {}: {error}", index + 1))?;
    }
    Ok(bindings)
}

pub(crate) fn init(path: PathBuf, cx: &mut App) {
    let job = cx.background_executor().spawn(async move {
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    });
    cx.spawn(async move |cx| match job.await {
        Ok(Some(text)) => {
            cx.update(|cx| {
                match compile(&text, crate::key_bindings(), |name, params| {
                    cx.build_action(name, params)
                        .map_err(|error| error.to_string())
                }) {
                    Ok(bindings) => {
                        cx.clear_key_bindings();
                        cx.bind_keys(bindings);
                    }
                    Err(error) => eprintln!("adoken: keymap.toml ignored: {error}"),
                }
            });
        }
        Ok(None) => {}
        Err(error) => eprintln!("adoken: couldn't read keymap.toml: {error}"),
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{KeyContext, Keymap, Keystroke};

    #[derive(Clone, PartialEq, Debug, gpui::Action)]
    #[action(namespace = keymap_test, no_json)]
    struct Jump {
        index: usize,
    }

    fn defaults() -> Vec<KeyBinding> {
        vec![
            KeyBinding::new("ctrl-1", Jump { index: 0 }, Some("Workspace")),
            KeyBinding::new("ctrl-2", Jump { index: 1 }, Some("Workspace")),
        ]
    }

    fn compile_test(text: &str) -> Result<Vec<KeyBinding>, String> {
        compile(text, defaults(), |_, _| Err("unknown action".into()))
    }

    fn target(map: &Keymap, keys: &str, context: &str) -> Option<usize> {
        let input = [Keystroke::parse(keys).unwrap()];
        let (bindings, _) = map.bindings_for_input(&input, &[KeyContext::parse(context).unwrap()]);
        bindings
            .first()
            .and_then(|binding| binding.action().as_any().downcast_ref::<Jump>())
            .map(|jump| jump.index)
    }

    #[test]
    fn remapping_one_number_keeps_other_targets_and_context() {
        let bindings = compile_test(
            r#"[[binding]]
context = "Workspace"
keys = "alt-2"
action = "keymap_test::Jump"
from = "ctrl-2"
"#,
        )
        .unwrap();
        let map = Keymap::new(bindings);
        assert_eq!(target(&map, "alt-2", "Workspace"), Some(1));
        assert_eq!(target(&map, "ctrl-2", "Workspace"), None);
        assert_eq!(target(&map, "ctrl-1", "Workspace"), Some(0));
        assert_eq!(target(&map, "alt-2", "Palette"), None);
    }

    #[test]
    fn ambiguous_numbered_action_requires_a_target() {
        assert!(
            compile_test(
                r#"[[binding]]
context = "Workspace"
keys = "alt-2"
action = "keymap_test::Jump"
"#
            )
            .is_err()
        );
    }

    #[test]
    fn malformed_override_does_not_apply_partial_bindings() {
        let result = compile_test(
            r#"[[binding]]
context = "Workspace"
keys = "alt-2"
action = "keymap_test::Jump"
from = "ctrl-2"
[[binding]]
keys = ""
action = "missing::Action"
"#,
        );
        assert!(result.is_err());
    }
}
