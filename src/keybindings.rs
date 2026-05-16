use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;

use relm4::gtk;
use relm4::gtk::gdk::Key;

use crate::configuration::APP_CONFIG;
use crate::sketch_board::KeyEventMsg;
use crate::style::Size;
use crate::tools::Tools;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutCommand {
    // generic
    ToggleToolbars,
    OpenGtkInspector,
    PanLeft,
    PanRight,
    PanUp,
    PanDown,
    DeleteSelection,
    RunActionsOnEscape,
    RunActionsOnEnter,

    // top toolbar
    OriginalScale,
    FitToWindow,
    ResetAll,
    SelectTool(Tools),
    Undo,
    Redo,
    SaveToClipboard,
    CopyFilepathToClipboard,
    SaveToFile,
    SaveToFileAs,

    // bottom toolbar
    SelectColorIndex(u64),
    CycleSize,
    SelectSize(Size),
    ToggleFill,
}

impl fmt::Display for ShortcutCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            // generic
            ShortcutCommand::OpenGtkInspector => "open-gtk-inspector",
            ShortcutCommand::PanLeft => "pan-left",
            ShortcutCommand::PanRight => "pan-right",
            ShortcutCommand::PanUp => "pan-up",
            ShortcutCommand::PanDown => "pan-down",
            ShortcutCommand::DeleteSelection => "delete-selection",
            ShortcutCommand::RunActionsOnEscape => "run-actions-on-escape",
            ShortcutCommand::RunActionsOnEnter => "run-actions-on-enter",
            ShortcutCommand::ToggleToolbars => "toggle-toolbars",

            // top toolbar
            ShortcutCommand::OriginalScale => "original-scale",
            ShortcutCommand::FitToWindow => "fit-to-window",
            ShortcutCommand::ResetAll => "reset-all",
            ShortcutCommand::Undo => "undo",
            ShortcutCommand::Redo => "redo",
            ShortcutCommand::SelectTool(tool) => {
                // write!(f, "select-tool-{}", tool)?;
                write!(f, "{}", tool.to_string().to_lowercase())?;
                return Ok(());
            }
            ShortcutCommand::SaveToFile => "save-to-file",
            ShortcutCommand::SaveToFileAs => "save-to-file-as",
            ShortcutCommand::SaveToClipboard => "save-to-clipboard",
            ShortcutCommand::CopyFilepathToClipboard => "copy-filepath-to-clipboard",

            // bottom toolbar
            ShortcutCommand::SelectColorIndex(index) => {
                write!(f, "select-color-index-{}", index + 1)?;
                return Ok(());
            }
            ShortcutCommand::CycleSize => "cycle-size",
            ShortcutCommand::SelectSize(size) => match size {
                Size::Small => "select-size-small",
                Size::Medium => "select-size-medium",
                Size::Large => "select-size-large",
            },
            ShortcutCommand::ToggleFill => "toggle-fill",
        };
        write!(f, "{}", name)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseCommandError;

impl FromStr for ShortcutCommand {
    type Err = ParseCommandError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            // generic
            "open-gtk-inspector" => Ok(ShortcutCommand::OpenGtkInspector),
            "toggle-toolbars" => Ok(ShortcutCommand::ToggleToolbars),
            "pan-left" => Ok(ShortcutCommand::PanLeft),
            "pan-right" => Ok(ShortcutCommand::PanRight),
            "pan-up" => Ok(ShortcutCommand::PanUp),
            "pan-down" => Ok(ShortcutCommand::PanDown),
            "delete-selection" => Ok(ShortcutCommand::DeleteSelection),
            "run-actions-on-escape" => Ok(ShortcutCommand::RunActionsOnEscape),
            "run-actions-on-enter" => Ok(ShortcutCommand::RunActionsOnEnter),

            // top toolbar
            "original-scale" => Ok(ShortcutCommand::OriginalScale),
            "fit-to-window" => Ok(ShortcutCommand::FitToWindow),
            "reset-all" => Ok(ShortcutCommand::ResetAll),
            "undo" => Ok(ShortcutCommand::Undo),
            "redo" => Ok(ShortcutCommand::Redo),
            "select-tool" => Ok(ShortcutCommand::SelectTool(Tools::Rectangle)),
            "save-to-file" => Ok(ShortcutCommand::SaveToFile),
            "save-to-file-as" => Ok(ShortcutCommand::SaveToFileAs),
            "save-to-clipboard" => Ok(ShortcutCommand::SaveToClipboard),
            "copy-filepath-to-clipboard" => Ok(ShortcutCommand::CopyFilepathToClipboard),

            // bottom toolbar
            text if text.starts_with("select-color-index-") => {
                let num_str = text.strip_prefix("select-color-index-").unwrap();

                if let Ok(num) = num_str.parse::<u64>() {
                    return Ok(ShortcutCommand::SelectColorIndex(num - 1));
                }
                Err(ParseCommandError)
            }
            "cycle-size" => Ok(ShortcutCommand::CycleSize),
            "select-size-small" => Ok(ShortcutCommand::SelectSize(Size::Small)),
            "select-size-medium" => Ok(ShortcutCommand::SelectSize(Size::Medium)),
            "select-size-large" => Ok(ShortcutCommand::SelectSize(Size::Large)),
            "toggle-fill" => Ok(ShortcutCommand::ToggleFill),

            _ => Err(ParseCommandError),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ShortcutRegistry {
    key_bindings: HashMap<String, ShortcutCommand>,
}

impl ShortcutRegistry {
    pub fn validate_keybinding(binding: &str) -> Result<(), String> {
        if let Some((keyval, modifier)) = gtk::accelerator_parse(binding) {
            if gtk::accelerator_valid(keyval, modifier) {
                Ok(())
            } else {
                Err(format!(
                    "Keybinding '{}' parsed successfully but not a valid hardware shortcut context.",
                    binding
                ))
            }
        } else {
            Err(format!(
                "Syntax Error: '{}' is not a recognized GTK accelerator string name.",
                binding
            ))
        }
    }

    fn add_key_binding(&mut self, key: &str, command: ShortcutCommand) -> bool {
        if let Err(err) = Self::validate_keybinding(key) {
            eprintln!(
                "Invalid key binding '{}' for command {:?}: {}",
                key, command, err
            );
            return false;
        }
        self.key_bindings.insert(key.to_string(), command);
        true
    }

    pub fn from_config() -> Self {
        static REGISTRY: OnceLock<ShortcutRegistry> = OnceLock::new();
        REGISTRY.get_or_init(Self::build_from_config).clone()
    }

    fn build_from_config() -> Self {
        println!("Building shortcut registry from config... ");
        let mut registry = Self::default();

        // generic
        registry.add_key_binding("<Control><Shift>d", ShortcutCommand::OpenGtkInspector);
        registry.add_key_binding("<Control><Shift>i", ShortcutCommand::OpenGtkInspector);
        registry.add_key_binding("<Control>t", ShortcutCommand::ToggleToolbars);
        registry.add_key_binding("<Alt>Left", ShortcutCommand::PanLeft);
        registry.add_key_binding("<Alt>Right", ShortcutCommand::PanRight);
        registry.add_key_binding("<Alt>Up", ShortcutCommand::PanUp);
        registry.add_key_binding("<Alt>Down", ShortcutCommand::PanDown);
        registry.add_key_binding("Delete", ShortcutCommand::DeleteSelection);
        registry.add_key_binding("<Shift>Delete", ShortcutCommand::ResetAll);
        registry.add_key_binding("Escape", ShortcutCommand::RunActionsOnEscape);
        registry.add_key_binding("Return", ShortcutCommand::RunActionsOnEnter);
        registry.add_key_binding("KP_Enter", ShortcutCommand::RunActionsOnEnter);

        // top toolbar
        registry.add_key_binding("<Control>1", ShortcutCommand::OriginalScale);
        registry.add_key_binding("<Control>2", ShortcutCommand::FitToWindow);
        registry.add_key_binding("<Control>z", ShortcutCommand::Undo);
        registry.add_key_binding("<Control>y", ShortcutCommand::Redo);
        registry.add_key_binding("p", ShortcutCommand::SelectTool(Tools::Pointer));
        registry.add_key_binding("c", ShortcutCommand::SelectTool(Tools::Crop));
        registry.add_key_binding("b", ShortcutCommand::SelectTool(Tools::Brush));
        registry.add_key_binding("i", ShortcutCommand::SelectTool(Tools::Line));
        registry.add_key_binding("z", ShortcutCommand::SelectTool(Tools::Arrow));
        registry.add_key_binding("r", ShortcutCommand::SelectTool(Tools::Rectangle));
        registry.add_key_binding("e", ShortcutCommand::SelectTool(Tools::Ellipse));
        registry.add_key_binding("t", ShortcutCommand::SelectTool(Tools::Text));
        registry.add_key_binding("m", ShortcutCommand::SelectTool(Tools::Marker));
        registry.add_key_binding("u", ShortcutCommand::SelectTool(Tools::Blur));
        registry.add_key_binding("g", ShortcutCommand::SelectTool(Tools::Highlight));
        registry.add_key_binding("<Control>c", ShortcutCommand::SaveToClipboard);
        registry.add_key_binding("<Control><Alt>c", ShortcutCommand::CopyFilepathToClipboard);
        registry.add_key_binding("<Control>s", ShortcutCommand::SaveToFile);
        registry.add_key_binding("<Control><Shift>s", ShortcutCommand::SaveToFileAs);

        // bottom toolbar
        for i in 0..10 {
            let key = i.to_string();
            registry.add_key_binding(&key, ShortcutCommand::SelectColorIndex(i));
        }

        registry.add_key_binding("minus", ShortcutCommand::CycleSize);
        registry.add_key_binding("f", ShortcutCommand::ToggleFill);

        // merge with config keybinds, allowing config to override defaults
        for (key, tool_or_command) in APP_CONFIG.read().keybinds() {
            if let Ok(tool) = Tools::from_str(tool_or_command.as_str()) {
                registry.add_key_binding(key, ShortcutCommand::SelectTool(tool));
                // println!("Registered key binding from config: {key} -> SelectTool({tool:?})");
            } else if let Ok(tool) = Tools::from_str(key.as_str()) {
                registry.add_key_binding(tool_or_command, ShortcutCommand::SelectTool(tool));
                eprintln!("Deprecated syntax for key binding: {key} = \"{tool_or_command}\"");
                eprintln!("Please update the config to use  : \"{tool_or_command}\" = \"{key}\"");
                // println!(
                //     "Registered key binding from config: {tool_or_command} -> SelectTool({tool:?})"
                // );
            } else if let Ok(command) = ShortcutCommand::from_str(tool_or_command.as_str()) {
                registry.add_key_binding(key, command);
                // println!("Registered key binding from config: {key} -> {command:?}");
            } else if tool_or_command == "none" {
                registry.key_bindings.remove(key);
                // println!("Key unbound by config: '{key}'");
            } else {
                eprintln!("Unknown tool or command in config for key '{key}': '{tool_or_command}'");
            }
        }

        registry
    }

    pub fn get_command_for_key_event(&self, event: &KeyEventMsg) -> Option<ShortcutCommand> {
        let key = gtk::accelerator_name(event.key, event.modifier).to_string();

        let modifier_only = matches!(
            event.key,
            Key::Control_L
                | Key::Control_R
                | Key::Shift_L
                | Key::Shift_R
                | Key::Alt_L
                | Key::Alt_R
                | Key::Meta_L
                | Key::Meta_R
                | Key::Super_L
                | Key::Super_R
        );
        // println!("   Key pressed: {keys}"); // FIXME remove this debug print
        if let Some(command) = self.key_bindings.get(&key) {
            // println!("   Matched shortcut command: {command:?}"); // FIXME remove this debug print
            Some(*command)
        } else if !modifier_only {
            eprintln!("Key {key} in not bound to a command or tool");
            None
        } else {
            None
        }
    }

    pub fn get_binding_for_command(&self, command: ShortcutCommand) -> Option<String> {
        self.key_bindings.iter().find_map(|(binding, cmd)| {
            if *cmd == command {
                Some(Self::format_binding_for_hint(binding))
            } else {
                None
            }
        })
    }

    fn format_binding_for_hint(binding: &str) -> String {
        let mut rest = binding;
        let mut parts: Vec<String> = Vec::new();

        while rest.starts_with('<') {
            let Some(end) = rest.find('>') else {
                break;
            };
            let token = &rest[1..end];
            parts.push(match token {
                "Control" => "Ctrl".to_string(),
                "Shift" => "Shift".to_string(),
                "Alt" => "Alt".to_string(),
                other => other.to_string(),
            });
            rest = &rest[end + 1..];
        }

        let key = rest.trim_end_matches('>');
        if !key.is_empty() {
            let key_label = match key {
                single if single.chars().count() == 1 => single.to_uppercase(),
                other => other.to_string(),
            };
            parts.push(key_label);
        }

        parts.join("+")
    }
}
