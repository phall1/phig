//! Canonical semantic command catalog shared by config, palette, and input adapters.

use super::{Action, PaletteCommand};

/// A semantic command's stable configuration name and human-facing palette label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDescriptor {
    pub semantic_name: Option<&'static str>,
    pub palette_label: &'static str,
    pub action: Action,
}

macro_rules! command {
    ($semantic:literal, $label:literal, $action:expr) => {
        CommandDescriptor {
            semantic_name: Some($semantic),
            palette_label: $label,
            action: $action,
        }
    };
    ($label:literal, $action:expr) => {
        CommandDescriptor {
            semantic_name: None,
            palette_label: $label,
            action: $action,
        }
    };
}

/// The single catalog for configurable action names and palette presentation.
pub const COMMANDS: &[CommandDescriptor] = &[
    command!("move-down", "Move down", Action::Move(1)),
    command!("move-up", "Move up", Action::Move(-1)),
    command!("page-down", "Page down", Action::Page(1)),
    command!("page-up", "Page up", Action::Page(-1)),
    command!("first", "First item", Action::First),
    command!("last", "Last item", Action::Last),
    command!("open", "Inspect commit", Action::Open),
    command!("back", "Back", Action::Back),
    command!("quit", "Quit", Action::Quit),
    command!("toggle-preview", "Toggle preview", Action::TogglePreview),
    command!("toggle-focus", "Toggle focus", Action::ToggleFocus),
    command!("search", "Search", Action::StartSearch),
    command!("palette", "Open command palette", Action::StartPalette),
    command!(
        "file-picker",
        "Open changed-file picker",
        Action::StartFilePicker
    ),
    command!("help", "Show help", Action::ToggleHelp),
    command!("next-match", "Next search match", Action::NextMatch),
    command!(
        "previous-match",
        "Previous search match",
        Action::PreviousMatch
    ),
    command!("next-hunk", "Next hunk", Action::NextHunk(1)),
    command!("previous-hunk", "Previous hunk", Action::NextHunk(-1)),
    command!("next-file", "Next changed file", Action::NextFile(1)),
    command!(
        "previous-file",
        "Previous changed file",
        Action::NextFile(-1)
    ),
    command!("next-parent", "Next merge parent", Action::NextParent),
    command!("view-log", "View history", Action::ViewLog),
    command!("view-refs", "View refs", Action::ViewRefs),
    command!("view-status", "View status", Action::ViewStatus),
    command!("view-tree", "View tree", Action::ViewTree),
    command!("view-blame", "View blame", Action::ViewBlame),
    command!("view-stash", "View stashes", Action::ViewStash),
    command!("mark", "Mark comparison endpoint", Action::Mark),
    command!("compare", "Compare revisions", Action::StartCompare),
    command!("swap-compare", "Swap comparison sides", Action::SwapCompare),
    command!(
        "toggle-compare-mode",
        "Toggle merge-base comparison",
        Action::ToggleCompareMode
    ),
    command!(
        "toggle-status-diff",
        "Toggle staged/unstaged diff",
        Action::ToggleStatusDiff
    ),
    command!("ascend", "Ascend tree", Action::Ascend),
    command!("copy-selection", "Copy selection", Action::CopySelection),
    command!("redraw", "Redraw terminal", Action::Redraw),
    command!("Retry failed request", Action::RetryFailed),
    command!("Dismiss errors", Action::DismissErrors),
];

impl Action {
    pub fn semantic_name(&self) -> &'static str {
        COMMANDS
            .iter()
            .find_map(|command| {
                (command.action == *self)
                    .then_some(command.semantic_name)
                    .flatten()
            })
            .unwrap_or("internal")
    }

    pub fn from_semantic_name(value: &str) -> Option<Self> {
        COMMANDS.iter().find_map(|command| {
            (command.semantic_name == Some(value)).then(|| command.action.clone())
        })
    }
}

pub fn palette_commands(query: &str) -> Vec<PaletteCommand> {
    let needle = query.to_lowercase();
    COMMANDS
        .iter()
        .filter(|command| {
            needle.is_empty() || command.palette_label.to_lowercase().contains(&needle)
        })
        .map(|command| PaletteCommand {
            name: command.palette_label,
            action: command.action.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{Action, COMMANDS, palette_commands};

    #[test]
    fn semantic_catalog_has_unique_round_tripping_names() {
        let configurable = COMMANDS
            .iter()
            .filter_map(|command| command.semantic_name.map(|name| (name, &command.action)))
            .collect::<Vec<_>>();
        let names = configurable
            .iter()
            .map(|(name, _)| *name)
            .collect::<HashSet<_>>();
        assert_eq!(names.len(), configurable.len());
        for (name, action) in configurable {
            assert_eq!(action.semantic_name(), name);
            assert_eq!(Action::from_semantic_name(name).as_ref(), Some(action));
        }
    }

    #[test]
    fn palette_is_derived_from_the_same_catalog() {
        assert_eq!(palette_commands("").len(), COMMANDS.len());
        assert_eq!(palette_commands("retry")[0].action, Action::RetryFailed);
    }
}
