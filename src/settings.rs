//! Persisted options (`@AppStorage` in the Swift app), stored in `NSUserDefaults` under the same keys.

use crate::platform::ns_string;
use fastfind_core::SortOption;
use objc2_foundation::NSUserDefaults;
use std::path::PathBuf;

const DIRECTORY: &str = "savedDirectoryPath";
const SORT: &str = "selectedSortOption";
const REVERSE: &str = "isSortReverse";
const BY_FOLDER: &str = "isCheckboxByFolderSelected";
const BY_FOLDER_DOUBLE: &str = "isCheckboxByFolderDoubleSelected";
const CONVERT_FULL_WIDTH: &str = "isConvertFullWidth";
const VJDECK_ENABLED: &str = "isVJDeckImportEnabled";
const VJDECK_NUMBERS: &str = "vjDeckNumbers";
/// No UI: `defaults write BadaGames.FastFind autoUpdate -bool NO` turns automatic updates off.
const AUTO_UPDATE: &str = "autoUpdate";

/// Automatic updates are on unless turned off with `AUTO_UPDATE`.
pub fn auto_update() -> bool {
    bool_or(&NSUserDefaults::standardUserDefaults(), AUTO_UPDATE, true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The saved folder as stored, even when it is not there right now (an unmounted drive):
    /// saving other options must not forget it.
    pub directory: Option<PathBuf>,
    pub sort: SortOption,
    pub reverse: bool,
    pub by_folder: bool,
    pub by_folder_double: bool,
    pub convert_full_width: bool,
    pub vjdeck_enabled: bool,
    pub vjdeck_numbers: String,
}

fn bool_or(defaults: &NSUserDefaults, name: &str, default: bool) -> bool {
    let k = ns_string(name);
    if defaults.objectForKey(&k).is_some() {
        defaults.boolForKey(&k)
    } else {
        default
    }
}

impl Settings {
    pub fn load() -> Self {
        let defaults = NSUserDefaults::standardUserDefaults();
        let directory = defaults
            .stringForKey(&ns_string(DIRECTORY))
            .map(|s| PathBuf::from(s.to_string()));
        Settings {
            directory,
            sort: SortOption::from_index(defaults.integerForKey(&ns_string(SORT)) as i64),
            reverse: bool_or(&defaults, REVERSE, false),
            by_folder: bool_or(&defaults, BY_FOLDER, true),
            by_folder_double: bool_or(&defaults, BY_FOLDER_DOUBLE, true),
            convert_full_width: bool_or(&defaults, CONVERT_FULL_WIDTH, true),
            vjdeck_enabled: bool_or(&defaults, VJDECK_ENABLED, true),
            vjdeck_numbers: defaults
                .stringForKey(&ns_string(VJDECK_NUMBERS))
                .map(|s| s.to_string())
                .unwrap_or_else(|| "1,2".to_string()),
        }
    }

    pub fn save(&self) {
        let defaults = NSUserDefaults::standardUserDefaults();
        match &self.directory {
            Some(dir) => unsafe {
                defaults.setObject_forKey(Some(&ns_string(&dir.to_string_lossy())), &ns_string(DIRECTORY))
            },
            None => defaults.removeObjectForKey(&ns_string(DIRECTORY)),
        }
        defaults.setInteger_forKey(self.sort as isize, &ns_string(SORT));
        defaults.setBool_forKey(self.reverse, &ns_string(REVERSE));
        defaults.setBool_forKey(self.by_folder, &ns_string(BY_FOLDER));
        defaults.setBool_forKey(self.by_folder_double, &ns_string(BY_FOLDER_DOUBLE));
        defaults.setBool_forKey(self.convert_full_width, &ns_string(CONVERT_FULL_WIDTH));
        defaults.setBool_forKey(self.vjdeck_enabled, &ns_string(VJDECK_ENABLED));
        unsafe { defaults.setObject_forKey(Some(&ns_string(&self.vjdeck_numbers)), &ns_string(VJDECK_NUMBERS)) };
    }
}
