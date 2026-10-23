//! Spec 0019 AC-S3: the review's and the list's keys against the whole app
//! keymap, under the context stacks the shell builds.

use gpui::{KeyBinding, KeyContext, Keymap, Keystroke};

use super::{LIST_CONTEXT, pr};

/// The shell's root context, the parent of every section's view.
const WORKSPACE: &str = "Workspace";
/// Keys the shell owns everywhere (⌘K row menu, ⌘[ / ⌘] history, ⌘⇧A
/// activity, ⌘P search): nothing in the review may shadow them.
const RESERVED: [&str; 5] = [
    "secondary-k",
    "secondary-[",
    "secondary-]",
    "secondary-shift-a",
    "secondary-p",
];

fn stack(contexts: &[&str]) -> Vec<KeyContext> {
    contexts
        .iter()
        .map(|c| KeyContext::parse(c).expect("context"))
        .collect()
}

fn input(binding: &KeyBinding) -> Vec<Keystroke> {
    binding
        .keystrokes()
        .iter()
        .map(|k| k.inner().clone())
        .collect()
}

fn describe(binding: &KeyBinding) -> String {
    let keys: Vec<String> = input(binding).iter().map(|k| k.to_string()).collect();
    format!(
        "{} → {} ({})",
        keys.join(" "),
        binding.action().name(),
        binding
            .predicate()
            .map_or_else(|| "global".to_owned(), |p| p.to_string())
    )
}

/// Bindings live at `contexts`: global ones and those whose predicate
/// holds somewhere on the stack.
fn live<'a>(bindings: &'a [KeyBinding], contexts: &[KeyContext]) -> Vec<&'a KeyBinding> {
    bindings
        .iter()
        .filter(|b| b.predicate().is_none_or(|p| p.depth_of(contexts).is_some()))
        .collect()
}

#[test]
fn ac_s3_review_and_list_keys_share_no_keystroke_and_shadow_no_shell_key() {
    let bindings = crate::key_bindings();
    let keymap = Keymap::new(bindings.clone());
    let workspace = stack(&[WORKSPACE]);
    for own in [LIST_CONTEXT, pr::KEY_CONTEXT] {
        let contexts = stack(&[WORKSPACE, own]);
        let live = live(&bindings, &contexts);
        let mine: Vec<&KeyBinding> = live
            .iter()
            .copied()
            .filter(|b| b.predicate().is_some_and(|p| p.to_string() == own))
            .collect();
        assert!(mine.len() > 10, "{own} has its keys");

        let mut clashes = Vec::new();
        for binding in &mine {
            let keys = input(binding);
            for other in &live {
                if !std::ptr::eq(*binding, *other) && other.match_keystrokes(&keys) == Some(false) {
                    clashes.push(format!("{} vs {}", describe(binding), describe(other)));
                }
            }
        }
        assert!(clashes.is_empty(), "{own}: shared keystrokes {clashes:#?}");

        for keys in RESERVED {
            let typed = [Keystroke::parse(keys).expect("keystroke")];
            assert!(
                !mine.iter().any(|b| b.match_keystrokes(&typed).is_some()),
                "{own} binds {keys}"
            );
            let shell = keymap.bindings_for_input(&typed, &workspace).0;
            let here = keymap.bindings_for_input(&typed, &contexts).0;
            let (Some(shell), Some(here)) = (shell.first(), here.first()) else {
                panic!("{keys} is bound in the shell and reaches {own}");
            };
            assert_eq!(
                here.action().name(),
                shell.action().name(),
                "{keys} in {own} runs the shell's action"
            );
        }
    }
}
