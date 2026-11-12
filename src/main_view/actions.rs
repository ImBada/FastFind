//! What the context menu, the keyboard and drops do to files: open, copy, rename, trash, move.

use super::*;

impl MainView {
    pub(super) fn copy_names(&mut self, cx: &mut Context<Self>) {
        let items = self.selected_items();
        if items.is_empty() {
            return;
        }
        Self::copy_lines(items.iter().map(|i| i.disk_name()), cx);
    }

    /// One line per item on the clipboard. Names are the real ones on disk: the list shows `:` as
    /// `/` and may show full-width text half-width, which matches no file when pasted.
    pub(super) fn copy_lines(lines: impl IntoIterator<Item = String>, cx: &mut Context<Self>) {
        let text = lines.into_iter().collect::<Vec<_>>().join("\n");
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
    }

    /// Items just renamed (`(from, to)`): they stay selected, and in view, under their new names.
    pub fn follow_renames(&mut self, renamed: &[(PathBuf, PathBuf)]) {
        for (from, to) in renamed {
            self.renamed
                .insert(nfc(&from.to_string_lossy()).into(), nfc(&to.to_string_lossy()).into());
        }
    }

    /// The results on screen.
    pub fn results(&self) -> Arc<Vec<Arc<FileInfo>>> {
        self.results.clone()
    }

    pub(super) fn after_file_action(&mut self, cx: &mut Context<Self>) {
        self.search_all(None, true, cx);
    }

    pub(super) fn context_entries(&mut self) -> Vec<MenuEntry> {
        self.menu_actions.clear();
        self.menu_targets.clear();
        let at = self.context_row.as_ref().and_then(|path| self.positions.get(path));
        let Some(item) = at.and_then(|&ix| self.results.get(ix)).cloned() else {
            return Vec::new();
        };
        self.menu_targets = if self.selection.contains(&item.path) {
            self.selected_items()
        } else {
            vec![item.clone()]
        };
        self.menu_clicked = Some(item);

        let decks = if self.settings.vjdeck_enabled {
            file_ops::parse_deck_numbers(&self.settings.vjdeck_numbers)
        } else {
            Vec::new()
        };
        // (row, its action, the actions of its submenu's rows)
        let mut rows: Vec<(MenuEntry, Option<MenuAction>, Vec<MenuAction>)> = vec![
            (
                MenuEntry::item(tr("Open")).symbol("arrow.up.right.square"),
                Some(MenuAction::Open),
                vec![],
            ),
            (
                MenuEntry::item(tr("Show Info")).symbol("info.circle"),
                Some(MenuAction::ShowInfo),
                vec![],
            ),
            (MenuEntry::separator(), None, vec![]),
            (
                MenuEntry::item(tr("Reveal in Finder")).symbol("folder"),
                Some(MenuAction::Reveal),
                vec![],
            ),
            (
                MenuEntry::item(tr("Copy Full Path")).symbol("doc.on.doc"),
                Some(MenuAction::CopyPath),
                vec![],
            ),
            (
                MenuEntry::item(tr("Copy Filename Only")).symbol("doc.on.doc"),
                Some(MenuAction::CopyName),
                vec![],
            ),
        ];
        if !decks.is_empty() {
            rows.push((MenuEntry::separator(), None, vec![]));
            rows.push((
                MenuEntry::item("Import to VJDeck").submenu(decks.iter().map(|d| MenuEntry::item(format!("Deck {d}")))),
                None,
                decks.iter().map(|&d| MenuAction::Deck(d)).collect(),
            ));
        }
        rows.extend([
            (MenuEntry::separator(), None, vec![]),
            (
                MenuEntry::item("Batch Rename").symbol("pencil.and.list.clipboard"),
                Some(MenuAction::BatchRename),
                vec![],
            ),
            (
                MenuEntry::item(tr("Rename")).symbol("pencil"),
                Some(MenuAction::Rename),
                vec![],
            ),
            (
                MenuEntry::item(tr("Move to Trash")).symbol("trash"),
                Some(MenuAction::Trash),
                vec![],
            ),
        ]);
        // The menu numbers its rows depth-first, so a submenu's rows follow it.
        let mut entries = Vec::with_capacity(rows.len());
        for (entry, action, submenu) in rows {
            entries.push(entry);
            self.menu_actions.push(action);
            self.menu_actions.extend(submenu.into_iter().map(Some));
        }
        entries
    }

    pub(super) fn perform_menu(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(action) = self.menu_actions.get(index).copied().flatten() else {
            return;
        };
        let targets = std::mem::take(&mut self.menu_targets);
        let clicked = self.menu_clicked.take();
        let paths: Vec<PathBuf> = targets.iter().map(|t| t.os_path.clone()).collect();
        match action {
            MenuAction::Open => platform::open_paths(&paths),
            MenuAction::ShowInfo => {
                cx.spawn(async move |_, _| platform::show_info(&paths)).detach();
            }
            MenuAction::Reveal => platform::reveal_in_finder(&paths),
            MenuAction::CopyPath => {
                Self::copy_lines(targets.iter().map(|t| t.os_path.to_string_lossy().into_owned()), cx)
            }
            MenuAction::CopyName => Self::copy_lines(targets.iter().map(|t| t.disk_name()), cx),
            MenuAction::Deck(deck) => file_ops::vjdeck_import(&targets, deck),
            MenuAction::BatchRename => {
                sheets::open_find_replace(window, cx);
            }
            MenuAction::Rename => {
                if let Some(item) = clicked.or_else(|| targets.first().cloned()) {
                    sheets::open_rename(window, cx, item.os_path.clone());
                }
            }
            MenuAction::Trash => self.confirm_trash(targets, cx),
        }
    }

    pub(super) fn confirm_trash(&mut self, items: Vec<Arc<FileInfo>>, cx: &mut Context<Self>) {
        let index = self.index.clone();
        cx.spawn(async move |this, cx| {
            let ok = platform::confirm(
                tr("Are you sure you want to move the selected file(s) to the Trash?"),
                tr("Move to Trash"),
                tr("Cancel"),
            );
            if !ok {
                return;
            }
            cx.background_executor()
                .spawn(async move { file_ops::trash_items(&index, &items) })
                .await;
            let _ = this.update(cx, |this, cx| this.after_file_action(cx));
        })
        .detach();
    }

    /// `FileOps.dropCopyFilesToDirectory`.
    pub(super) fn drop_into(&mut self, folder: Arc<FileInfo>, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if !folder.is_dir {
            return;
        }
        self.copying += 1;
        cx.notify();
        let index = self.index.clone();
        // The whole drop, the overwrite question between its two halves included.
        let busy = file_ops::Busy::start();
        cx.spawn(async move |this, cx| {
            let _busy = busy;
            let (fresh, existing) = file_ops::plan_drop(&paths, &folder.os_path);
            if !fresh.is_empty() {
                let index = index.clone();
                cx.background_executor()
                    .spawn(async move { fresh.iter().for_each(|job| file_ops::copy_or_move(&index, job)) })
                    .await;
                let _ = this.update(cx, |this, cx| this.after_file_action(cx));
            }
            if !existing.is_empty() {
                let names = file_ops::overwrite_names(&existing);
                let overwrite = !names.is_empty()
                    && platform::confirm(
                        &format!(
                            "{}\n\n{}",
                            tr("Files already exists, are you sure you want to overwrite it?"),
                            names.join("\n")
                        ),
                        tr("Overwrite"),
                        tr("Cancel"),
                    );
                if overwrite {
                    cx.background_executor()
                        .spawn(async move { existing.iter().for_each(|job| file_ops::copy_or_move(&index, job)) })
                        .await;
                    let _ = this.update(cx, |this, cx| this.after_file_action(cx));
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.copying = this.copying.saturating_sub(1);
                cx.notify();
            });
        })
        .detach();
    }

    /// Original-filename mode: rename from the row's own text field.
    pub(super) fn inline_rename(&mut self, item: Arc<FileInfo>, name: String, cx: &mut Context<Self>) {
        self.edits.remove(&item.path);
        // Both normalised: an unchanged name stored decomposed (NFD) is no rename.
        if item.disk_name() == nfc(&name) {
            return;
        }
        let Some(target) = file_ops::rename_target(&item.os_path, &name) else {
            // Not a file name: the field goes back to the real one.
            cx.notify();
            return;
        };
        sheets::rename_then_refresh(vec![(item.os_path.clone(), target)], Some(cx.entity().downgrade()), cx);
    }
}
