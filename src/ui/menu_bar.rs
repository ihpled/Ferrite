//! Classic text menu bar (File, Edit, Paragraph, Format, View, Themes, Help).
//!
//! An alternative to the icon [`Ribbon`](super::Ribbon), selected with
//! `Settings::toolbar_style`. It renders no side effects itself: every entry
//! returns a [`RibbonAction`], so the app handles menu clicks through the same
//! code path as ribbon buttons (including selection capture for formatting).
//! Generic commands go through [`RibbonAction::Command`], which reuses the
//! command palette dispatcher, and shortcut hints are read from the user's
//! [`KeyboardShortcuts`] so rebinding a key updates the menu too.

use crate::config::{KeyboardShortcuts, ShortcutCommand, Theme, ViewMode};
use crate::markdown::formatting::MarkdownFormatCommand;
use crate::state::FileType;
use crate::ui::icons::phosphor_rich_text;
use crate::ui::RibbonAction;
use eframe::egui::{self, Color32, Ui};
use egui_phosphor::regular::CHECK;
use rust_i18n::t;
use std::path::PathBuf;

/// Read-only state the menu bar needs to label, check and enable its entries.
pub struct MenuBarState<'a> {
    pub shortcuts: &'a KeyboardShortcuts,
    pub recent_files: &'a [PathBuf],
    pub has_editor: bool,
    pub file_type: FileType,
    pub is_workspace_mode: bool,
    pub theme: Theme,
    pub view_mode: ViewMode,
    pub auto_save_enabled: bool,
    pub show_line_numbers: bool,
    pub word_wrap: bool,
    pub outline_enabled: bool,
    pub file_tree_visible: bool,
    pub sync_scroll_enabled: bool,
    pub pipeline_enabled: bool,
    pub zen_mode: bool,
}

/// Classic text menu bar.
#[derive(Debug, Clone, Copy, Default)]
pub struct MenuBar;

impl MenuBar {
    pub fn new() -> Self {
        Self
    }

    /// Render the menu bar and return the action of the clicked entry, if any.
    pub fn show(&self, ui: &mut Ui, s: &MenuBarState<'_>) -> Option<RibbonAction> {
        let mut action = None;
        egui::MenuBar::new().ui(ui, |ui| {
            let mut m = Menu {
                s,
                action: &mut action,
            };
            ui.menu_button(t!("menu_bar.file.label").to_string(), |ui| m.file_menu(ui));
            ui.menu_button(t!("menu_bar.edit.label").to_string(), |ui| m.edit_menu(ui));
            ui.menu_button(t!("menu_bar.paragraph.label").to_string(), |ui| {
                m.paragraph_menu(ui)
            });
            ui.menu_button(t!("menu_bar.format.label").to_string(), |ui| {
                m.format_menu(ui)
            });
            ui.menu_button(t!("menu_bar.view.label").to_string(), |ui| m.view_menu(ui));
            ui.menu_button(t!("menu_bar.themes.label").to_string(), |ui| {
                m.themes_menu(ui)
            });
            ui.menu_button(t!("menu_bar.help.label").to_string(), |ui| m.help_menu(ui));
        });
        action
    }
}

/// Per-frame menu builder: holds the state and the action slot to fill.
struct Menu<'s, 'a> {
    s: &'s MenuBarState<'a>,
    action: &'s mut Option<RibbonAction>,
}

impl Menu<'_, '_> {
    // ── Entry helpers ────────────────────────────────────────────────────────

    fn shortcut_text(&self, cmd: Option<ShortcutCommand>) -> Option<String> {
        let binding = self.s.shortcuts.get(cmd?);
        binding.has_modifiers().then(|| binding.display_string())
    }

    /// A plain entry. `cmd` only supplies the shortcut hint.
    fn entry(
        &mut self,
        ui: &mut Ui,
        label: String,
        cmd: Option<ShortcutCommand>,
        enabled: bool,
        action: RibbonAction,
    ) {
        let mut button = egui::Button::new(label);
        if let Some(text) = self.shortcut_text(cmd) {
            button = button.shortcut_text(text);
        }
        if ui.add_enabled(enabled, button).clicked() {
            *self.action = Some(action);
        }
    }

    /// An entry with a check-mark gutter (checked or blank), for toggles and radio items.
    fn check_entry(
        &mut self,
        ui: &mut Ui,
        checked: bool,
        label: String,
        cmd: Option<ShortcutCommand>,
        enabled: bool,
        action: RibbonAction,
    ) {
        // Always draw the glyph so labels stay aligned; hide it when unchecked.
        let mark = phosphor_rich_text(CHECK, 12.0);
        let mark = if checked {
            mark
        } else {
            mark.color(Color32::TRANSPARENT)
        };
        let mut button = egui::Button::new((mark, label));
        if let Some(text) = self.shortcut_text(cmd) {
            button = button.shortcut_text(text);
        }
        if ui.add_enabled(enabled, button).clicked() {
            *self.action = Some(action);
        }
    }

    /// Entry that runs a shortcut command through the shared dispatcher.
    fn command(&mut self, ui: &mut Ui, label: String, cmd: ShortcutCommand, enabled: bool) {
        self.entry(ui, label, Some(cmd), enabled, RibbonAction::Command(cmd));
    }

    /// Entry that applies a Markdown formatting command to the current selection.
    fn format(
        &mut self,
        ui: &mut Ui,
        label: String,
        cmd: Option<ShortcutCommand>,
        format: MarkdownFormatCommand,
    ) {
        let enabled = self.s.has_editor && self.s.file_type.is_markdown();
        self.entry(ui, label, cmd, enabled, RibbonAction::Format(format));
    }

    // ── Menus ────────────────────────────────────────────────────────────────

    fn file_menu(&mut self, ui: &mut Ui) {
        let has_editor = self.s.has_editor;

        self.command(
            ui,
            t!("menu_bar.file.new").to_string(),
            ShortcutCommand::New,
            true,
        );
        self.command(
            ui,
            t!("menu.window.new_window").to_string(),
            ShortcutCommand::NewWindow,
            true,
        );
        ui.separator();

        self.command(
            ui,
            t!("menu.file.open").to_string(),
            ShortcutCommand::Open,
            true,
        );
        if self.s.is_workspace_mode {
            self.command(
                ui,
                t!("menu_bar.file.close_folder").to_string(),
                ShortcutCommand::CloseWorkspace,
                true,
            );
        } else {
            self.command(
                ui,
                t!("menu_bar.file.open_folder").to_string(),
                ShortcutCommand::OpenWorkspace,
                true,
            );
        }
        ui.separator();

        // The quick file switcher searches the open folder (same as the ribbon,
        // which only shows it in workspace mode).
        self.command(
            ui,
            t!("menu_bar.file.open_quickly").to_string(),
            ShortcutCommand::QuickOpen,
            self.s.is_workspace_mode,
        );
        let recent = self.s.recent_files;
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button(t!("menu.file.recent").to_string(), |ui| {
                for (index, path) in recent.iter().enumerate() {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    let response = ui.button(name).on_hover_text(path.display().to_string());
                    if response.clicked() {
                        *self.action = Some(RibbonAction::OpenRecent(index));
                    }
                }
            });
        });
        self.command(
            ui,
            t!("menu.file.reload").to_string(),
            ShortcutCommand::Reload,
            has_editor,
        );
        ui.separator();

        self.command(
            ui,
            t!("menu.file.save").to_string(),
            ShortcutCommand::Save,
            has_editor,
        );
        self.command(
            ui,
            t!("menu.file.save_as").to_string(),
            ShortcutCommand::SaveAs,
            has_editor,
        );
        self.check_entry(
            ui,
            self.s.auto_save_enabled,
            t!("menu_bar.file.auto_save").to_string(),
            None,
            has_editor,
            RibbonAction::ToggleAutoSave,
        );
        ui.separator();

        let is_markdown = has_editor && self.s.file_type.is_markdown();
        ui.add_enabled_ui(is_markdown, |ui| {
            ui.menu_button(t!("menu.file.export").to_string(), |ui| {
                self.command(
                    ui,
                    t!("menu.file.export_html").to_string(),
                    ShortcutCommand::ExportHtml,
                    true,
                );
                self.command(
                    ui,
                    t!("menu.file.export_pdf").to_string(),
                    ShortcutCommand::ExportPdf,
                    true,
                );
                self.entry(
                    ui,
                    t!("menu.file.export_clipboard").to_string(),
                    None,
                    true,
                    RibbonAction::CopyAsHtml,
                );
            });
        });
        self.command(
            ui,
            t!("menu.file.print_preview").to_string(),
            ShortcutCommand::PrintPreview,
            is_markdown,
        );
        ui.separator();

        self.command(
            ui,
            t!("menu_bar.file.preferences").to_string(),
            ShortcutCommand::OpenSettings,
            true,
        );
        ui.separator();

        self.command(
            ui,
            t!("menu_bar.file.close").to_string(),
            ShortcutCommand::CloseTab,
            has_editor,
        );
    }

    fn edit_menu(&mut self, ui: &mut Ui) {
        let has_editor = self.s.has_editor;

        self.command(
            ui,
            t!("menu_bar.edit.undo").to_string(),
            ShortcutCommand::Undo,
            has_editor,
        );
        self.command(
            ui,
            t!("menu_bar.edit.redo").to_string(),
            ShortcutCommand::Redo,
            has_editor,
        );
        ui.separator();

        self.command(
            ui,
            t!("menu_bar.edit.delete_line").to_string(),
            ShortcutCommand::DeleteLine,
            has_editor,
        );
        self.command(
            ui,
            t!("menu_bar.edit.duplicate_line").to_string(),
            ShortcutCommand::DuplicateLine,
            has_editor,
        );
        self.command(
            ui,
            t!("menu_bar.edit.select_next_occurrence").to_string(),
            ShortcutCommand::SelectNextOccurrence,
            has_editor,
        );
        ui.separator();

        self.command(
            ui,
            t!("menu_bar.edit.find").to_string(),
            ShortcutCommand::Find,
            has_editor,
        );
        self.command(
            ui,
            t!("menu_bar.edit.find_replace").to_string(),
            ShortcutCommand::FindReplace,
            has_editor,
        );
        self.command(
            ui,
            t!("menu_bar.edit.find_next").to_string(),
            ShortcutCommand::FindNext,
            has_editor,
        );
        self.command(
            ui,
            t!("menu_bar.edit.find_prev").to_string(),
            ShortcutCommand::FindPrev,
            has_editor,
        );
        self.command(
            ui,
            t!("menu_bar.edit.search_in_files").to_string(),
            ShortcutCommand::SearchInFiles,
            self.s.is_workspace_mode,
        );
        ui.separator();

        self.command(
            ui,
            t!("menu_bar.edit.go_to_line").to_string(),
            ShortcutCommand::GoToLine,
            has_editor,
        );
        self.command(
            ui,
            t!("menu_bar.edit.command_palette").to_string(),
            ShortcutCommand::CommandPalette,
            true,
        );
    }

    fn paragraph_menu(&mut self, ui: &mut Ui) {
        const HEADINGS: [ShortcutCommand; 6] = [
            ShortcutCommand::FormatHeading1,
            ShortcutCommand::FormatHeading2,
            ShortcutCommand::FormatHeading3,
            ShortcutCommand::FormatHeading4,
            ShortcutCommand::FormatHeading5,
            ShortcutCommand::FormatHeading6,
        ];
        for (level, cmd) in (1u8..).zip(HEADINGS) {
            self.format(
                ui,
                t!("menu_bar.paragraph.heading", level = level).to_string(),
                Some(cmd),
                MarkdownFormatCommand::Heading(level),
            );
        }
        ui.separator();

        self.format(
            ui,
            t!("menu_bar.paragraph.code_block").to_string(),
            Some(ShortcutCommand::FormatCodeBlock),
            MarkdownFormatCommand::CodeBlock,
        );
        self.format(
            ui,
            t!("menu_bar.paragraph.quote").to_string(),
            Some(ShortcutCommand::FormatBlockquote),
            MarkdownFormatCommand::Blockquote,
        );
        ui.separator();

        self.format(
            ui,
            t!("menu_bar.paragraph.ordered_list").to_string(),
            Some(ShortcutCommand::FormatNumberedList),
            MarkdownFormatCommand::NumberedList,
        );
        self.format(
            ui,
            t!("menu_bar.paragraph.unordered_list").to_string(),
            Some(ShortcutCommand::FormatBulletList),
            MarkdownFormatCommand::BulletList,
        );
        ui.separator();

        let is_markdown = self.s.has_editor && self.s.file_type.is_markdown();
        self.command(
            ui,
            t!("menu_bar.paragraph.toc").to_string(),
            ShortcutCommand::InsertToc,
            is_markdown,
        );

        if self.s.file_type.is_structured() {
            ui.separator();
            let has_editor = self.s.has_editor;
            self.entry(
                ui,
                t!("ribbon.format_document").to_string(),
                None,
                has_editor,
                RibbonAction::FormatDocument,
            );
            self.entry(
                ui,
                t!("ribbon.validate_syntax").to_string(),
                None,
                has_editor,
                RibbonAction::ValidateSyntax,
            );
        }
    }

    fn format_menu(&mut self, ui: &mut Ui) {
        self.format(
            ui,
            t!("menu_bar.format.bold").to_string(),
            Some(ShortcutCommand::FormatBold),
            MarkdownFormatCommand::Bold,
        );
        self.format(
            ui,
            t!("menu_bar.format.italic").to_string(),
            Some(ShortcutCommand::FormatItalic),
            MarkdownFormatCommand::Italic,
        );
        self.format(
            ui,
            t!("menu_bar.format.strikethrough").to_string(),
            None,
            MarkdownFormatCommand::Strikethrough,
        );
        self.format(
            ui,
            t!("menu_bar.format.code").to_string(),
            Some(ShortcutCommand::FormatInlineCode),
            MarkdownFormatCommand::InlineCode,
        );
        ui.separator();

        self.format(
            ui,
            t!("menu_bar.format.link").to_string(),
            Some(ShortcutCommand::FormatLink),
            MarkdownFormatCommand::Link,
        );
        self.format(
            ui,
            t!("menu_bar.format.image").to_string(),
            Some(ShortcutCommand::FormatImage),
            MarkdownFormatCommand::Image,
        );
    }

    fn view_menu(&mut self, ui: &mut Ui) {
        let s = self.s;
        let has_editor = s.has_editor;
        let is_fullscreen = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));

        self.check_entry(
            ui,
            s.view_mode != ViewMode::Raw,
            t!("menu_bar.view.rendered_mode").to_string(),
            Some(ShortcutCommand::ToggleViewMode),
            has_editor,
            RibbonAction::Command(ShortcutCommand::ToggleViewMode),
        );
        ui.separator();

        self.check_entry(
            ui,
            s.file_tree_visible,
            t!("menu_bar.view.sidebar").to_string(),
            Some(ShortcutCommand::ToggleFileTree),
            s.is_workspace_mode,
            RibbonAction::Command(ShortcutCommand::ToggleFileTree),
        );
        self.check_entry(
            ui,
            s.outline_enabled,
            t!("menu_bar.view.outline").to_string(),
            Some(ShortcutCommand::ToggleOutline),
            true,
            RibbonAction::Command(ShortcutCommand::ToggleOutline),
        );
        self.check_entry(
            ui,
            false,
            t!("menu_bar.view.terminal").to_string(),
            Some(ShortcutCommand::ToggleTerminal),
            true,
            RibbonAction::Command(ShortcutCommand::ToggleTerminal),
        );
        self.check_entry(
            ui,
            false,
            t!("menu_bar.view.productivity").to_string(),
            Some(ShortcutCommand::ToggleProductivityHub),
            true,
            RibbonAction::Command(ShortcutCommand::ToggleProductivityHub),
        );
        if matches!(s.file_type, FileType::Json | FileType::Yaml) {
            self.check_entry(
                ui,
                false,
                t!("ribbon.pipeline").to_string(),
                Some(ShortcutCommand::TogglePipeline),
                has_editor && s.pipeline_enabled,
                RibbonAction::Command(ShortcutCommand::TogglePipeline),
            );
        }
        ui.separator();

        self.check_entry(
            ui,
            s.show_line_numbers,
            t!("menu_bar.view.line_numbers").to_string(),
            None,
            true,
            RibbonAction::ToggleLineNumbers,
        );
        self.check_entry(
            ui,
            s.word_wrap,
            t!("menu_bar.view.word_wrap").to_string(),
            Some(ShortcutCommand::ToggleWordWrap),
            true,
            RibbonAction::Command(ShortcutCommand::ToggleWordWrap),
        );
        self.check_entry(
            ui,
            s.sync_scroll_enabled,
            t!("menu_bar.view.sync_scroll").to_string(),
            None,
            true,
            RibbonAction::ToggleSyncScroll,
        );
        ui.separator();

        self.check_entry(
            ui,
            s.zen_mode,
            t!("menu_bar.view.zen_mode").to_string(),
            Some(ShortcutCommand::ToggleZenMode),
            true,
            RibbonAction::Command(ShortcutCommand::ToggleZenMode),
        );
        self.check_entry(
            ui,
            is_fullscreen,
            t!("menu_bar.view.fullscreen").to_string(),
            Some(ShortcutCommand::ToggleFullscreen),
            true,
            RibbonAction::Command(ShortcutCommand::ToggleFullscreen),
        );
        ui.separator();

        self.command(
            ui,
            t!("menu_bar.view.zoom_in").to_string(),
            ShortcutCommand::ZoomIn,
            true,
        );
        self.command(
            ui,
            t!("menu_bar.view.zoom_out").to_string(),
            ShortcutCommand::ZoomOut,
            true,
        );
        self.command(
            ui,
            t!("menu_bar.view.actual_size").to_string(),
            ShortcutCommand::ResetZoom,
            true,
        );
        ui.separator();

        self.command(
            ui,
            t!("menu_bar.view.next_tab").to_string(),
            ShortcutCommand::NextTab,
            has_editor,
        );
        self.command(
            ui,
            t!("menu_bar.view.prev_tab").to_string(),
            ShortcutCommand::PrevTab,
            has_editor,
        );
        self.entry(
            ui,
            t!("menu_bar.view.back_to_previous_tab").to_string(),
            None,
            has_editor,
            RibbonAction::PreviousTab,
        );
    }

    fn themes_menu(&mut self, ui: &mut Ui) {
        let current = self.s.theme;
        for (theme, label) in [
            (Theme::Light, t!("menu_bar.themes.light")),
            (Theme::Dark, t!("menu_bar.themes.dark")),
            (Theme::System, t!("menu_bar.themes.system")),
        ] {
            self.check_entry(
                ui,
                current == theme,
                label.to_string(),
                None,
                true,
                RibbonAction::SetTheme(theme),
            );
        }
        ui.separator();
        self.command(
            ui,
            t!("menu_bar.themes.cycle").to_string(),
            ShortcutCommand::CycleTheme,
            true,
        );
    }

    fn help_menu(&mut self, ui: &mut Ui) {
        self.entry(
            ui,
            t!("menu_bar.help.markdown_reference").to_string(),
            None,
            true,
            RibbonAction::ToggleMarkdownCheatsheet,
        );
        ui.separator();
        self.command(
            ui,
            t!("menu_bar.help.about").to_string(),
            ShortcutCommand::OpenAbout,
            true,
        );
    }
}
