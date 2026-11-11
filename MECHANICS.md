# Rules and values taken from the original

Functions read with Ghidra: 0x1005d5cb0, 0x100306dc0, 0x10053c360, 0x100ae99e0, 0x1000518e0, 0x100dffce0, 0x100e65c20. Callers of log strings missing from the built-in index were found by scanning the code for RIP-relative loads of the string address. (Google Drive for desktop 133.0.2, macOS x86_64)

Sources: the installer was unpacked (xar → pbzx → cpio). The main program
`Google Drive.app/Contents/MacOS/Google Drive` was decompiled with Ghidra; the
settings window is a script (`Resources/preferences_dialog.bin`); every UI
text, in all languages including Vietnamese, is stored inside the main program.

## Network speed limits ("Cài đặt băng thông")
| Value | Number | Source |
|---|---|---|
| Download rate field: off unless its checkbox is ticked | – | preferences_dialog.bin, `#download-rate-checkbox` handler |
| Download rate min / max | 1 / 100000000 kB/s | preferences_dialog.bin, `c.min=1;c.max=1E8` for `#download-rate-input` |
| Upload rate min / max | 1 / 100000000 kB/s | preferences_dialog.bin, `#upload-rate-input` |
| Unit shown | kB/giây | Vietnamese string table in main program |
| Stored value → bytes per second | value × 1024 (`<< 10`) | function 0x1005d5cb0 (UpdateBandwidthThrottler, logs "Download speed throttled at N kbps") |
| A limit applies only when its flag bit is set (0x10 download, 0x20 upload); either one turns the throttler on | function 0x1005d5cb0 |

The rebuild passes the limits to rclone as `--bwlimit UPk:DOWNk` (rclone's `k` is 1024 bytes, same as the original).

## Cache ("Cài đặt bộ nhớ đệm")
| Value | Number | Source |
|---|---|---|
| Cache size limit min / max | 1 / 100000000 | preferences_dialog.bin, `#cache-size-limit-input` (`a.min=1,a.max=1E8`) |
| Unit | GB | **estimated** — the English string table has no unit next to "Cache size limit" (checked the full table; only the speed fields carry "kB/s" in some languages) |
| Capacity is handled in bytes; logged as MB (`bytes >> 20`) | – | function 0x100dffce0 (`UpdateContentCacheCapacity`, cello_fs.cc) |
| Cache use is reported as MiB and as `used × 100 / capacity` (whole percent, rounded down); evictable share the same way | – | function 0x100e65c20 (content_cache_capacity_heuristic.cc) |
| Limit off by default; limit doesn't apply to files waiting to sync or kept offline | – | English string "Limit won't apply to files that need to sync or files you make available offline." |

## Pause
| Value | Source |
|---|---|
| Pause is a *reason* "user-paused" added to (pause) or removed from (resume) a persisted set, so it survives a restart; a failure to persist is only logged ("Persisting pause state of … failed with") | function 0x100306dc0 (`SetSyncingPaused`, apps/drive/fs/ui/client.cc) |
| The new state is applied to every account in turn | function 0x10053c360 (loops the account map, calls 0x100551820 for each) |
| On startup an account starts paused when its stored reasons contain "user-paused" | function 0x1005d5cb0 |
| Resuming in mirror mode polls for changes straight away ("Mirror polling for changes after unpausing failed with …") | function 0x100ae99e0 (apps/drive/fs/core/mirror/mirror.cc) |
| While paused: status text "Đã tạm dừng đồng bộ hóa. Còn # tệp cần đồng bộ hóa." | Vietnamese string table |

## Google Drive modes ("Cách đồng bộ hóa Drive của tôi")
| Mode | Meaning | Rebuild |
|---|---|---|
| Truy cập trực tuyến tệp (stream, listed first = default) | files stay in the cloud, visible as a folder; opened files are cached | `rclone mount` with a full file cache |
| Phản chiếu tệp (mirror) | every file is stored in the cloud and on the computer | `rclone bisync` between the folder and Drive, repeated |

## Folders from your computer ("Thư mục trên máy tính của bạn")
| Option | Meaning (from the original's text) | Rebuild |
|---|---|---|
| Đồng bộ hóa với Google Drive | upload everything; edits and deletions are synced | `rclone sync` folder → Drive "Máy tính Linux/<máy>/<thư mục>" |
| Sao lưu vào Google Photos | only photos and videos; deletions are not synced | `rclone copy` with photo/video filter to a Google Photos connection |

## Other
| Value | Number | Source |
|---|---|---|
| Accounts at once (original) | 4 | string "Kết nối và truy cập các tệp từ 4 tài khoản Google cùng một lúc" |
| Network storage backed up every | 6 hours | string "Network-attached storage backed up every 6 hours" (not used: no NAS support) |
| Launch on login | on by default | flag default `gui_autostart_option = true` (0x103f5224f) |
| Mirror re-check interval | 120 s | **estimated** (original reacts to changes instantly) |
| Folder backup re-check interval | 300 s | **estimated** |
| Streaming folder | `<home>/Library/CloudStorage/GoogleDrive-<account>` on macOS → `<home>/GoogleDrive-<account>` on Linux | string 0x1040700fe used in function 0x1000518e0 (domain_controller.mm) |
| Attaching the streaming folder | up to 10 attempts (gives up when the count passes 9), 2 s apart (delay 2 000 000 000 ns) | function 0x1000518e0 ("PostDelay the add domain count: n/10") |
| Activity list kept | 50 items | flag default `item_events_buffer_size = 50` (0x103f49eed) |

## Main window (version 133 "Home" window — `home_dialog.css`, `home_dialog__en_gb.js`)
| Value | Number | Source |
|---|---|---|
| Navigation column width | 272 px | `.saqWhd{grid-template-columns:272px max-content}` |
| Top bar height | 64 px | `.gUWKnc{height:64px}` |
| Navigation items, in order | Home, Sync activity, Notifications, Connected folders, Offline files, Shared with me; Settings at the bottom | nav builder functions in home_dialog__en_gb.js |
| Home cards | Sync status, Notifications, Quick links (Open Drive folder, Open Drive web, Add more folders to sync), Storage usage, Folders connected to Drive, Files available offline | labels in home_dialog__en_gb.js ("syncing-status-bento-box" ids) |
| Sync activity table | Name · File size · Status | home_dialog__en_gb.js |
| Settings groups | General settings (Appearance, Launch on login, Proxy settings, Bandwidth settings…), Account settings (My Drive syncing options, Google Drive streaming location…) | home_dialog__en_gb.js |
| Colours (light / dark) | primary #0b57d0 / #a8c7fa, surface container #f0f4f9 / #1e1f20, selected #c2e7ff / #004a77, text #1f1f1f / #e3e3e3 | `--gm3-sys-color-*` in home_dialog.css |
| Default language | English (Vietnamese optional, using the original's Vietnamese wording where it exists) | person's request |

## Photo / video types, ignored files, streaming-folder check (second decompile pass)
| Rule | Value | Source |
|---|---|---|
| Photo types backed up | jpg, jpeg, jpe, png, gif, heif, heic, tif, tiff, webp, bmp, dib, ico, jp2, j2c, jpf, j2k, jpx | default of flag 45371132, loaded in function 0x1009c22f0 (string 0x1040a2936) |
| RAW photo types ("Upload RAW files") | dng, raw, arw, crw, cr2, cr3, dcr, dc2, erf, kc2, kdc, k25, mdc, mef, mos, mrw, nef, nrw, orf, pef, qtk, raf, rdc, rw2, srf, srw, sr2, x3f, 3fr | same string, tail at 0x1040a2983; option name `photos_upload_raw_files` |
| RAW default | off | `photos_upload_raw_files` (0x103f77acc, policy PhotosUploadRAWFiles) is a bool with no default value, so it starts off |
| Video types backed up | 3g2, 3gp, asf, avi, divx, flv, m2t, m2ts, m4v, mkv, mmv, mng, mod, mov, mp4, mpe, mpg, mpeg, mts, ogv, qt, tod, webm, wmv | default of flag 45371155, function 0x1009c22f0 (string 0x1040a2a00) |
| File names never synced (11 patterns) | `~$*`, `.~lock.*#`, `*.swp`, `.DS_Store`, `._*`, `.apdisk`, `.localized`, `Icon\r`, `.Trash`, `.Spotlight-V100`, `.fseventsd` | function 0x1013684a0 (ignore_patterns.cc) |
| Streaming folder check | missing → OK (created); not a folder → refuse; no access → refuse; folder must be empty except entries whose name starts with "." | function 0x10152f980 (`CheckForValidDirMountPoint`, path_util.cc) |
| A refused folder is not retried | the attach gives up instead of looping | 0x10152f980 return codes feed 0x1000518e0 |
| Sign-in wait limit | 300 s | flag default `browser_auth_wait_for_client_timeout_ms = 300000` (0x103f52044) |

## Mirror rules and folder overlap (third pass)
| Rule | Value | Source |
|---|---|---|
| A synced computer folder may not be inside (or contain) the Google Drive folder | refused with "The folder cannot be within {MOUNT_POINT} which is being used by {EMAIL}" | original string table only; the code that shows it runs in the settings page script, not in a native function found so far |
| Mirror: file deleted on one side and present in the last agreed list → delete on the other side | Drive copy goes to Drive's trash (kept 30 days, "Items will be permanently deleted after 30 days"); local copy goes to the desktop trash | original strings "Moved to trash" / 30-day text; rclone 1.60 bisync refused this case (max-delete 50% and empty-folder guard), so the app plans each file itself |
| Mirror: changed on both sides (or two different files with one name on the first run) | Drive's version is put in place; the computer's version is never overwritten — it is moved to a "Lost and Found" folder and the person is told "Can’t sync file, moved it to “Lost and Found” folder." | function 0x100bfc6a0 (`MoveNewFileIntoPlace`, mirror/local/local_writer.cc: refuses with ALREADY_EXISTS "Conflicting file exists at … is unavailable"), function 0x100671870 (`DeconflictCopy`: copies the item "to lost and found"), English string table line "Can’t sync file, moved it to …Lost and Found… folder." |
| Downloaded mirror files | written to a temporary file, then moved into place | function 0x100bfc6a0 (checks `filesystem::Exists(args.temp_file_path)` then moves) — the rebuild relies on rclone's copy for this |
| Two accounts may not use the same Drive folder | exact path match refused (status ALREADY_EXISTS, code 0xe6) | function 0x10139f100 (`CheckUIPreferencesForDuplicateMountPoint`, preferences_manager.cc) — the rebuild has one account, so it can't happen |
| Mirror: never pulls "Folders from your computer" backups | they live outside My Drive | original keeps them in the separate "Computers" area |
| Mirror: a failed file listing is never treated as "everything deleted" | sync is skipped and retried | safety rule of this rebuild |

## Features added to close the "Different or missing" list
| Rule | Value | Source |
|---|---|---|
| Accounts at once | 4 (selected one is edited in Settings; each has its own syncing options) | Vietnamese string "Kết nối và truy cập các tệp từ 4 tài khoản Google cùng một lúc"; home window strings "Add another account", "Maximum number of accounts reached", "Disconnect an account to add another" |
| Streaming folder layout | "My Drive" and "Shared drives" inside the account's folder | original streaming drive layout (strings "My Drive", "Shared drives"); built with rclone combine, two levels because rclone 1.60 refuses "/" in one level |
| Cached files kept | until space is needed (`--vfs-cache-max-age 8760h`) | original keeps cached files; rclone's default would drop them after 1 h |
| Files kept available offline | re-read every 30 min so the cache holds them | **estimated** interval |
| USB device prompt | "Add “{DEVICE_NAME}”" with "Add device" / "Not now" and "Remember my choice for this device"; setting "Prompt me to back up devices"; list "Ignored USB Devices" with "Stop ignoring all" | original string table |
| USB devices | backed up while plugged in, synced as soon as they appear | original string "USB devices backed up when connected" |
| Network storage (NFS, SMB, SSHFS, WebDAV) | backed up every 6 hours | original string "Network-attached storage backed up every 6 hours" |
| Local changes | uploaded with no idle wait (the 2-second manager loop groups changes arriving together) | flag default `upload_idle_delay_msec = 0` (descriptor at 0x103f7b250) |
| Search | names across all connected accounts; list re-read when Drive reports changes, at most every 5 min | flag default `file_provider_string_search_expiration_time_ms = 300000` (0x103f4ff6b); replaces macOS Spotlight |
| Tray icon | Open / Pause or Resume syncing / Preferences / Quit Google Drive; closing the window keeps running | original's menu-bar icon behaviour |
| Newer rclone | downloaded from downloads.rclone.org, SHA-256 checked against SHA256SUMS, kept in the app's own folder | needed for Google Photos after Google's March 2025 change |

## Built-in setting defaults (fourth pass)
The program's settings are protocol-buffer fields whose defaults are stored next to their names
(field option `default_value`). Read directly from the descriptors in the main program:

| Setting | Default | Address | Used for |
|---|---|---|---|
| `multi_account_max_accounts` | 4 | 0x103f529e6 | accounts at once |
| `changelog_polling_interval_ms` | 10000 | 0x103f4b64e | Drive's change list is read every 10 s (mirror starts at once when something changed; streaming uses `--poll-interval 10s`) |
| `poll_for_changes_debounce_interval_ms` | 0 | 0x103f4fc48 | no extra wait after a change is seen |
| `upload_idle_delay_msec` | 0 | 0x103f7b250 | local changes upload without waiting |
| `reconnected_nas_polling_interval_seconds` | 60 | 0x103f528f1 | a network folder that comes back is synced 60 s later |
| `min_free_space_for_writes_kbytes` / `min_local_free_space_for_writes` | 512000 | 0x103f79dcd / 0x103f4baab | mirror downloads stop when less than 500 MB would stay free ("Not enough disk space" / "You don’t have enough disk space to sync some files") |
| `file_provider_string_search_expiration_time_ms` | 300000 | 0x103f4ff6b | search list refresh |
| `content_cache_max_kbytes` | 1000000000 | 0x103f4ba71 | built-in cache ceiling (~1 TB); doesn't settle the unit of the settings field |
| `changelog_backup_poll_interval_ms` | 11111000 | 0x103f4bf79 | (backup poll when push notifications fail; not needed — the app always polls every 10 s) |
| `min_delay_before_initial_scan_ms` | 120000 | 0x103f79dff | (not used: the app scans at start) |

Still estimated: the mirror safety re-check every 120 s when the change list can't be read, re-reading
offline files every 30 min, and "Prompt me to back up devices" being on by default (no default found).

## Account card (fifth pass)
| Rule | Value | Source |
|---|---|---|
| Profile picture size | 256 px, square crop: "=s256-c" | function 0x1003d5d60 (home_dialog.cc builds the account list; string 0x103b03f30) |
| Existing picture options | options after "=" that start with c, h, s or w (crop, height, size, width) are dropped, the rest kept after "=s256-c" | function 0x1003d5d60 (`(c - 'c')` tested against mask 0x110021) |
| Storage with total 0 | logged "Bytes total is 0"; only the used amount is shown | function 0x1003d5d60 |
| Accounts listed | one entry per signed-in account, each with picture, quota and status | function 0x1003d5d60 (loops the account list) |

## Notifications and quitting (sixth pass)
| Rule | Value | Source |
|---|---|---|
| "Don’t show this again" | a show/hide flag per notification type and account, kept in a database table `notifications (type, account_token, show)` | function 0x1009435b0 (root_preference_db.cc, writes `INSERT … ON CONFLICT … DO UPDATE SET show`); rebuild keeps the hidden types in settings |
| Activity items kept | 50 | flag default `item_events_buffer_size` (0x103f49eed) |
| Sync status changes shown | 15 | flag default `max_syncing_status_change_events` (0x103f49ebf) — not used separately |
| Quit while files sync | asks "Quit Google Drive?" / "Files are still syncing. Your changes won’t sync unless Google Drive is running." | original string table |
| Quit during system logout | no confirmation | flag default `confirm_quit_during_system_logout = false` (0x103f5321b); rebuild quits on SIGTERM/SIGHUP/SIGINT after releasing streaming folders and saving history |
| Notification and activity history kept between runs | yes, in the app's own file | asked for by the person; the original's notification list itself wasn't found in its database (only the show flags) |

## Quitting (seventh pass)
| Rule | Value | Source |
|---|---|---|
| Quit request while syncing | "push changes and quit": a flag is set (a second request is ignored, "Ignoring duplicate request to push changes and quit."), every account is told to upload what's pending | function 0x1003070c0 (`RequestPushChangesAndQuit`, client.cc) |
| When it actually quits | only when no uploads are pending ("Quitting because there are no pending uploads.") | function 0x100312a10 (`MaybePerformSafeQuit`, client.cc) |
| Rebuild | the "Quit Google Drive?" dialog offers "Finish syncing, then quit" (waits until nothing is uploading, then quits), "Quit now" and "Cancel" | from the two functions above |
| Closing the window | Drive keeps running in the tray (the original is a menu-bar app with no main window to close); can be turned off in Settings | person's request; original behaviour |
| Wayland | the app uses the X11 connection (XWayland) when available, because Wayland doesn't let an app hide its own window | rebuild detail |

## Sync activity history (eighth pass)
| Rule | Value | Source |
|---|---|---|
| Last event time per item | stored as (item_id, event_id, timestamp=now) in table `last_item_event_time`, upserted on every event | function 0x100849940 (`RecordItemEvent`, file_provider/sqlite_working_set.cc); queries `SELECT timestamp FROM last_item_event_time WHERE item_id = ? AND event_id = ?` |
| Events for items no longer present | ignored ("Warning: event … for non-present item … ignored.") | function 0x100849940 |
| Item removed | its rows are deleted (`DELETE FROM last_item_event_time where item_id = ?`) | string table; deletion path in function 0x100856e10 ("Failed to delete item events items in db") |
| Items shown | last 50 events | flag default `item_events_buffer_size = 50` (0x103f49eed) |
| Rebuild | the mirror state keeps `synced_at` per file (now for files copied in a run, kept for unchanged ones, dropped for removed ones); with no saved history, Sync activity is filled from these times and skips files no longer on disk | from the rules above |

## Devices and single instance (ninth pass)
| Rule | Value | Source |
|---|---|---|
| Devices are known by ID | table `media (media_id, name, last_mount_point, fs_type, device_type, capacity, ignored)`; a device keeps its settings wherever it is mounted | string table; rebuild uses the filesystem UUID (`findmnt`) as media_id |
| "Not now" + "Remember my choice" | sets `ignored = 1` for that media_id ("Ignoring media with id: …") | function 0x10093f270 (`UPDATE media SET ignored = ? WHERE media_id = ?`, root_preference_db.cc) |
| Ignored list shown in Settings | name (hover: last mount point); "Remove ignored device" / "Stop ignoring all" clear the flag | `SELECT media_id, name, last_mount_point FROM media WHERE ignored = ?` |
| A backed-up device mounted somewhere new | followed by its ID (`last_mount_point` updated) | `UPDATE media SET name = …, last_mount_point = … WHERE media_id = …` |
| One running copy | lock file opened (on failure removed and retried), then exclusive non-blocking `flock`; if held: "Lock file is held by another process" and this copy stops | function 0x1001c76a0 (single_instance_mutex_posix.cc); rebuild then asks the running copy to show its window |

## Remaining defaults (tenth pass)
| Setting | Default | Address | Used for |
|---|---|---|---|
| `browser_auth_wait_for_client_timeout_ms` | 300000 | 0x103f52044 | browser sign-in gives up after 5 min |
| `gui_autostart_option` | true | 0x103f5224f | "Launch on login" starts on |
| `photos_upload_raw_files` | (no default → false) | 0x103f77acc | "Upload RAW files" starts off |
| `photos_max_upload_retries_offline` | 3 | 0x103f4cd76 | Google Photos uploads are tried 3 times |
| `max_photo_total_cache_size_bytes` | 1073741824 | 0x103f4cc4c | (not used: the rebuild has no photo cache) |

## File manager menu (missing-features pass 1)
| Rule | Value | Source |
|---|---|---|
| Menu entries | "Copy link to clipboard", "Share with Google Drive", "Manage versions with Google Drive", "Open with Google Drive", "Available offline", "Online only" | original string table (Finder menu) |
| Copy link result | "Link copied to the clipboard." | original string table |
| Not possible | "Can’t complete action" | original string table |
| Rebuild | Dolphin service menu (Google Drive submenu) and GNOME Files scripts, installed at start (setting "Show Google Drive in the file manager menu", on by default); each runs the app with `--action`, passed to the running copy over its single-instance socket; Drive ID read with `rclone lsjson --stat` | — |
| Not done yet | "Add shortcut to Google Drive", sync status badges on files | — |

## Links from the file menu (eleventh pass)
| Rule | Value | Source |
|---|---|---|
| "Copy link to clipboard" | the text the core supplies is put on the clipboard on the UI thread | function 0x100580500 (`CopyToClipboard`, core_controller.cc) |
| Which link | the item's own Drive link (`alternate_link` / v3 `webViewLink`, e.g. `https://drive.google.com/file/d/<id>/view?usp=drivesdk`) | field names `alternate_link`/`alternateLink` in the string table; rebuild reads `webViewLink` and falls back to a built address |
| "Share with Google Drive" | the core is asked for a share-dialog URL, then the share window opens even if that failed ("Failed to get URL for share dialog, status: …" is only logged) | function 0x10033b870 (client.cc); rebuild opens the file's page with `usp=sharing` in the browser, where Share is one click |

## Shared drive syncing (missing-features pass 2)
| Rule | Value | Source |
|---|---|---|
| Setting | "Shared drive syncing" in Account settings: each shared drive switched on or off ("Toggle shared drive"); messages "Shared drive syncing enabled/disabled", "Shared drive sync settings updated", "Loading shared drives" | home_dialog__en_gb.js strings |
| How many | no limit | flag default `max_shared_drives_to_sync = 0` (0x103f4eef2) |
| List of shared drives | fetched at start, then updated whenever Drive's change list reports changes | flag default `fetch_shared_drive_roots_on_init = true` (0x103f7bbf5); function 0x1017c53b0 (`ProcessSharedDriveChanges`, merge_task.cc); rebuild also re-reads it hourly as a safety net (**estimated**) |
| Drives no longer listed | removed from syncing after a successful list ("Removing hidden, unsynced or no longer existing shared drive: …"); a failed list changes nothing | function 0x10064bb90 (file_provider_server.cc) |
| Where mirrored shared drives go | "Shared drives (<account>)" folder next to the My Drive mirror folder, one folder per drive | rebuild choice (the original keeps them in its own drive) |
| Sync rules | same as My Drive mirroring (change list every 10 s, Lost and Found, trash, ignore list), with its own saved state | earlier sections |

## Ransomware protection (missing-features pass 3)
| Rule | Value | Source |
|---|---|---|
| Who detects it | Google's servers: the core reports it in the syncing status (a flag plus the alert time in ms) and the UI's `CheckForRansomwareAlert` acts on it | function 0x1002e0ea0 (apps/drive/fs/ui/client.cc, lines 0x8ea–0x98a); proto defaults `has_ransomware_alert = false` (0x103f622ca), `ransomware_alert_timestamp_ms = 0` (0x103f622ef) |
| Alert kept across restarts | a "ransomware-alert" sentinel file holding the account and alert time ("Failed to write ransomware alert sentinel file: …") | string 0x104081724, function 0x1002e0ea0 |
| An earlier clear wins | a "ransomware-cleared" or "ransomware-false" sentinel for the same account wins when it is no older than 3600 s (0xe10) before the alert's time ("ExistingRansomwareClearTakesPrecedence"); an older one is removed ("Failed to remove ransomware clear sentinel file") | function 0x1002e0ea0 |
| Signing out ends the alert | yes | symbol `HandleRansomwareAlertOnSignOut` |
| Notification | "Syncing paused: Ransomware detected on {0}" / "Multiple files were modified, and syncing was paused on {0} at {1}. If you don‘t recognize this activity, action needs to be taken to prevent further corruption." with "Take action", "I recognize it", "Allow sync"; after: "Cleared sync restriction" / "Google Drive cleared the sync restriction on {0}."; failure: "Can‘t clear sync restriction" | English string table (lines 20657–20668 of the string dump), Vietnamese table (89637–89650) |
| Dialog ("Take action") | "Ransomware detected: File syncing paused on {date} at {time}", "What is ransomware?", "Drive keeps old file versions for 25 days…", steps "Disconnect account and restore files" and "Clear files and resume syncing", "Learn more about ransomware recovery" | home_dialog__en_gb.js / home_dialog__vi.js (`ransomware-dialog-content`) |
| Rebuild's detection | the server signal can't be read through rclone or the public Drive API, so a Mirror pass that would push ≥ 50 rewritten-or-removed synced files that are also ≥ 10 % of the folder's synced files, or ≥ 500 at once, is held back before anything reaches Drive | **estimated** thresholds |
| Rebuild's "Clear files and resume syncing" | the changed and new files are moved to "Lost and Found/Changed files <date> <time>", Drive's untouched copies are downloaded back, a "ransomware-cleared" sentinel is written and syncing resumes; "I recognize it" → "Allow sync" writes "ransomware-false" and syncs the changes | rebuild choice following the original's steps |

## Diagnostics and logs (missing-features pass 4)
| Rule | Value | Source |
|---|---|---|
| Where | "Generate diagnostic info", "Enable verbose logging" / "Disable verbose logging" (help menu in the rebuild) | English and Vietnamese string tables |
| Runs on | one dedicated "diagnostic_info_generator_thread", made once and reused; one report at a time | function 0x100512ce0 (feedback_recorder.cc, GenerateDiagnosticInfo) |
| File name | "Google Drive Diagnostics - %Y-%m-%dT%H.%M.%E2S%Ez.zip", ":" replaced by "." | function 0x10094ba20 (diagnostic_util.cc) |
| Name clash | time moved on 40 ms (40000000 ns) and tried again; 16 attempts ("Can't create zip file in 16 attempts") | function 0x10094ba20 |
| Where it ends up | created in the temp folder, then moved to the desktop (MaybeMoveDiagnosticFileToDesktop); if that fails the temp file is kept | function 0x10094ba20 |
| Messages | "Generating diagnostic info" / "Diagnostic info is being generated. This may take a few minutes. Please wait…" / "Diagnostic info created" / "Your diagnostic has been created at “{FULL_PATH}”" / "Diagnostic info could not be created" | string tables |
| Log file | "structured_log.txt", at most 5120 KB per file, 30 files kept | flag defaults structured_log_max_file_size_kbytes (0x103f52384), structured_log_max_file_count (0x103f523d0); string "structured_log.txt" |
| Rebuild's report | about.txt (versions, system, desktop), connections.txt (connection names and types), config/settings.json, history.json and ransomware files, mirror-states.txt (counts only), Logs/; rclone.conf (sign-in tokens) is never included; stored (uncompressed) zip | rebuild choice |
| Verbose logging | rclone runs with -vv instead of -v and progress lines are logged too | rebuild choice |

## Sync status badges on files (missing-features pass 5)
| Rule | Value | Source |
|---|---|---|
| Badge identifiers | "pinned", "cloud", "mirror_root", "synced", "syncing", "blacklisted", "cse" (in that order; badge images pinned.icns, cloud.icns, syncing.icns, blacklisted.icns…) | FinderSyncExtension CFStrings 0x10154adc8–0x10154ae88, strings at 0x101510aa1; Resources folder of FinderSyncExtension.appex |
| Mirrored items | status 1 → "synced", 2 → "syncing", 3 → "blacklisted"; anything else gets no badge ("Unable to set badge for …") | -[DFSFinderSync setMirrorSyncStatusOverlay:] 0x100021ab0 |
| Which rule applies | the item's kind is looked up first: kind 1 (mirrored) asks the sync status; kind 3 (streamed) picks "pinned" when available offline, else "cloud", and "cse" for client-side encrypted files | -[DFSFinderSync requestBadgeIdentifierForURL:] 0x10001be80 |
| Rebuild's statuses | mirror folder root → mirror_root; ignored names (the 11 patterns) → blacklisted; a file whose size and time match the last synced list → synced; new or changed here, or moving now → syncing; a recent error on it → blacklisted; folders → syncing while a file below them moves; streamed → pinned/cloud from "Available offline"; connected folders show synced/syncing on the folder itself | rebuild choice following the rules above |
| How file managers ask | local socket $XDG_RUNTIME_DIR/drive-linux-$USER-status.sock: "Q <path>" lines and ".", one badge per line back; "WATCH" gets "CHANGED" when statuses may have changed (checked every 1.5 s, **estimated**) | rebuild choice (the original uses Finder's own extension IPC) |
| Dolphin | a KIO overlay-icon plugin (fm-plugins/dolphin, built by build.rs with Qt 6 and KF6 KIO) installed to ~/.local/lib/qt6/plugins/kf6/overlayicon, with QT_PLUGIN_PATH set for the next login in ~/.config/plasma-workspace/env/drive-linux.sh | rebuild |
| GNOME Files | a nautilus-python extension (works only when that package is installed) | rebuild |
| Emblem icons | the app's own drawings, named "drivelinux_<badge>" in hicolor/scalable/emblems; names have no "-" because KDE shortens unknown dashed names ("drive-linux-synced" → "drive") and would show another icon from the person's theme | rebuild; observed in Dolphin 25.12 |
| Losing the link to the app | every badge is taken down ("unpublish" on each tracked item), then the listener tries again after 5 s (dispatch_after 5000000000 ns), each try counted; after 6 tries it gives up ("Failed to RegisterProgressChangeListener. Max retry count exceeded."); badges are still asked for item by item | FinderSyncExtension FUN_1000244d0 (DFSFinderSync.mm line 0x13b/0x138) and FUN_100024b30 (`_registerPprogressListenerRetryCount` += 1); rebuild's Dolphin plugin does the same |

## Keeping the installed parts current (badges follow-up)
| Rule | Value | Source |
|---|---|---|
| Restart into a newer copy | when the app's program file was replaced on disk, it waits until nothing is syncing and no dialog is open, quits cleanly (streaming folders released, history saved) and starts the new copy in the same process, keeping the window hidden if it was; checked every 30 s (**estimated**) | rebuild choice; the original also restarts its core when needed ("Effective features or admin settings changed: restart required.", restart_on_browser_request = true @0x103f53fd5) |
| Dolphin plugin path | written both to ~/.config/plasma-workspace/env/drive-linux.sh and ~/.config/environment.d/90-drive-linux.conf; read at login, so Dolphin windows opened before then don't show badges | rebuild; observed on Plasma with Dolphin 25.12 (a running Dolphin passes new folders to its existing window) |

## Cache size limit field (settled)
| Rule | Value | Source |
|---|---|---|
| Unit shown beside the number | "GB" (the cache_size_limit_units string, the text field's suffix) | string tables: "GB" sits between "Cache size limit" and "Enter a value between 1 and {UPPER_LIMIT}." (e.g. 0x102a07c92); preferences_dialog.bin passes string 101 as the field's suffix |
| Allowed range | 1 to 100000000; the error text's {UPPER_LIMIT} is filled from the same constant for the cache and bandwidth fields | function 0x1002ce650 (UPPER_LIMIT ← 0x103ade640 / 0x103ade644 = 100000000); preferences_dialog.bin min=1, max=1E8 |
| Rebuild | the limit is entered in GB and passed to rclone as `--vfs-cache-max-size <n>G` | src/engine.rs ensure_mount |

## Add shortcut to Google Drive (missing-features pass 6)
| Rule | Value | Source |
|---|---|---|
| Flow | a save dialog asks where the shortcut goes; cancelling does nothing ("Save File Dialog cancelled"); a dialog error shows "Error adding shortcut" / "Failed to create shortcut" | function 0x10036aae0 (client.cc, lines 0x859–0x86e), strings 0x37b4/0x37b7 |
| Where it may go | only inside the account's Drive folder (path must start with "<mount point>/"); otherwise "Selected new shortcut item path … is not within mount point …" and "A shortcut cannot be added to this location" | function 0x10036aae0 (prefix compare with the mount point + "/"), string 0x37b5 |
| Name | picking a folder (empty name part) names the shortcut after the shared item | function 0x10036aae0 (appends the item's name when the chosen name is empty) |
| Targets refused by Drive | empty target ("Shortcut target id cannot be empty."), shared drive roots, trashed items | strings 0x10412bc4e, 0x10411dc03, 0x10411dc54 (functions 0x101b8de48 / 0x101b8df85) |
| Messages | "Add shortcut to Google Drive", "Creating shortcut", "Created a shortcut", "Shortcut added at …" | string tables; log string 0x104083fed |
| Rebuild | Shared with me rows get an "Add shortcut to Google Drive" button; the chosen local folder is mapped to its Drive folder (My Drive root → "root", otherwise its ID from `rclone lsjson --stat`), then Drive API files.create with mimeType application/vnd.google-apps.shortcut and shortcutDetails.targetId; Drive's own error message is shown | rebuild (Drive API v3) |
| Retry after "permission denied" | 3600000 ms | flag default shortcuts_permission_denied_retry_interval_ms @0x103f4c19a (not used: the rebuild makes one request per click) |

## Files kept available offline (pin refresh, settled)
| Rule | Value | Source |
|---|---|---|
| Who keeps them | a subscription task that finds pinned/shortcut-target items that aren't subscribed ("SubscriptionTask: Found unsubscribed pinned/shortcut target items: …") and asks for them again ("SubscriptionTask: Resubscribing item: … (id: …, title: …)") | FUN_101761b10 (apps/drive/cello/sync/subscription_task.cc, RunInternal / ShouldResubscribeItem) |
| How often it runs | throttled to once per 300000 ms | flag default subscription_task_throttle_time_ms @0x103f4d005 |
| How many per run | at most 50; more are cut ("Too many items to re-subscribe, trimmed to 50") | flag default subscription_task_max_operations @0x103f4d046, compared in FUN_101761b10 |
| Same item again | only after 14400000 ms | flag default resubscribe_retry_interval_msec @0x103f4c219 |
| Rebuild | every 300 s it reads at most 50 offline files into rclone's cache: never-read files first, then the longest ago, each again only after 4 h (src/engine.rs pins_due; replaces the earlier estimated 30-minute full re-read) | rebuild |

## Two-way sync: delete on one side, edit on the other (found while writing tests)
| Rule | Value | Source |
|---|---|---|
| Deleted on this computer, edited on Drive since the last sync | Drive's edited copy is downloaded again (not deleted) | rebuild choice: the original never deletes an item with changes it hasn't seen (it keeps conflicting content, MoveNewFileIntoPlace 0x100bfc6a0 / DeconflictCopy 0x100671870) |
| Deleted on Drive, edited on this computer since | this computer's copy is uploaded again (not deleted) | same |
| Checked by | src/mirror.rs test `an_edit_wins_over_a_delete_on_the_other_side` | — |

## Badges that went missing (follow-up)
| Rule | Value | Source |
|---|---|---|
| After the app restarts | the badges taken down when the link was lost come back once the plugin reconnects (it asks again for every item it still remembers) | rebuild; the original also takes badges down on a lost link (FinderSyncExtension FUN_1000244d0) and re-registers its listener |
| Retry count | a successful reconnect starts a new run of 6 tries (the app restarts itself on updates, which the original's long-lived core doesn't) | rebuild choice, departing from FUN_100024b30's never-reset counter |
| Item not answered | shown without a badge for now and asked again after 2 s (**estimated**), until the app answers | rebuild choice |
| Items remembered for updates | the newest 5000 (**estimated**); older ones are dropped one by one instead of all at once | rebuild choice |
| Checked by | the plugin loaded through QPluginLoader with a stand-in app that starts late and one that restarts: the badge appears late, is taken down, and comes back | manual harness |

## Periodic polls (settled)
| Rule | Value | Source |
|---|---|---|
| Poller | `PeriodicPoller(poll_period, min_backoff, max_backoff)`; the period must be above zero ("poll_period_ > absl::ZeroDuration()") | constructor 0x100ab3480 (apps/drive/fs/core/periodic_poller.cc line 0x5d) |
| Drive change list | every changelog_polling_interval_ms = 10000 ms (int32 field 7); after a failure it backs off to 2× (20 s), then up to 4× (40 s) | function 0x100ab32a0 (period from config +0x54, back-off = period×2 and period×4) |
| Backup poll | a second poller with a fixed period of changelog_backup_poll_interval_ms = 11111000 ms (int64 field 61), no back-off | function 0x100ab30e0 (period from config +0x338, min = max = period) |
| Rebuild | the change list backs off the same way (engine changes_poll_delay; this also eases Drive's "Quota exceeded" answers); a mirrored folder is fully re-checked every 11111 s while its local watch and Drive's change list both work, and every 120 s (**estimated**) when either doesn't (a watch that fails, e.g. at the system's inotify limit, is now noticed and logged) | src/engine.rs, src/settings.rs |

## Home notifications card (layout fix)
| Rule | Value | Source |
|---|---|---|
| Card | "Needs my attention" bento: width 360px, min-height 320px, max-height 370px; the list scrolls inside it | home_dialog.css `.QNY0bf{width:360px;min-height:320px;max-height:370px}`, `.HapGeb{height:100%}`; home_dialog__en_gb.js yBa/xBa ("Notifications list") |
| Rows | fixed height 77px each | home_dialog.css `.HapGeb{--gm3-data-table-row-item-container-height:77px}` |
| Rebuild | each notification is one 77px row (title one line, text at most two lines ending in "…"), three rows visible, the rest scroll; long words such as paths break inside the word so text can never widen a card (that is what pushed the cards out of line); the Notifications page wraps the same way | src/app.rs fit_text, NOTICE_ROW_H; test `notification_text_never_widens_its_card` |

## Folders from this computer: periodic walk (settled)
| Rule | Value | Source |
|---|---|---|
| What | each backed-up folder ("slurp root") is walked again on a timer, in addition to file-system change events | SlurpSyncRoot::PollPath FUN_10015a020 (apps/drive/fs/media/common/slurp_sync_root.cc lines 0xa8–0xb4) → FSMedia::PollPath 0x10013a470 → PollPathInternal / PollPathRepeatedly 0x10013b780 → walk task 0x10014a390 → "Finishing PolledPath for … with status" 0x10013fd60 (fs_media.cc) |
| How often | slurp_poll_path_frequency_min = 240 minutes | flag default @0x103f52a1d |
| Rebuild | connected folders are walked every 240 min while the file watcher covers them, every 300 s (**estimated**) when it doesn't; network folders keep "backed up every 6 hours" | src/settings.rs FOLDER_POLL_SECS, src/engine.rs |

## Name and logo
| Rule | Value | Source |
|---|---|---|
| Name shown | "Drive For Linux" (window, tray, notifications, menu, About); internal names stay `drive-linux` so settings carry over | person's request |
| Logo | assets/logo.svg, drawn for this project (a white cloud with a terminal prompt on a blue-to-cyan rounded tile); used in the app, as the window/tray icon (rendered with resvg) and as the menu icon | rebuild; Google's artwork is not used |
| Menu entry | ~/.local/share/applications/drive-linux.desktop with the icon in ~/.local/share/icons/hicolor/scalable/apps/drive-linux.svg | rebuild |

## Shared drive list refresh (settled)
| Rule | Value | Source |
|---|---|---|
| When the list is read | at start (fetch_shared_drive_roots_on_init = true), then whenever the change list carries a shared drive change; the original has no timer flag for it (only fetch_shared_drive_roots_on_init and skip_polling_team_drives = false exist) | flag defaults @0x103f7bbf5, @0x103f5bfda; ProcessSharedDriveChanges 0x1017c53b0 |
| Rebuild | the change list is now read with `changeType`: a "drive" change re-reads the shared drive list (and the streaming folder's "Shared drives"), file changes no longer do; a safety re-read follows the backup poll (11111000 ms) instead of the earlier estimated hourly one | src/engine.rs classify_changes / drive_changes; test `only_drive_changes_reread_the_shared_drive_list` |

## Photos backup waits for edits to settle (built-in decompiler pass, 2026-11-10)
| Rule | Value | Source |
|---|---|---|
| Bandwidth rule re-read | stored kB/s value `<< 10` (× 1024) when flag bit 0x10 (download) / 0x20 (upload) is set; logged "Download/Upload speed throttled at N kbps"; throttling also needs the "Bandwidth throttling support enabled" feature switch | function 0x1005d5cb0 (account_context.cc, UpdateBandwidthThrottler, lines 0xb6–0xbc); log text at 0x10408c7ca |
| A photo or video is backed up only after it has gone unchanged for the settle time; each new change restarts that file's wait | per file, the change time is stored in a time-ordered list; the action is scheduled at (earliest change + delay); a delay of 0 or less acts at once | function 0x100c7fbe0 (records the change, or acts at once when the delay ≤ 0); function 0x100c804f0 ("ScheduleAction", apps/drive/fs/core/photos/slurp_transporter/debouncer.cc, line 0x53) |
| Settle time | 300000 ms (5 min) | flag default `photos_debounce_edit_time_ms` (0x103f4cdee) — the only Photos debounce setting |
| Other Photos defaults (not used: rclone handles them) | throttled back-off 1000 ms → 300000 ms; failed-file list retried every 86400000 ms (24 h); catalog sync every 900000 ms | `photos_throttled_initial_backoff_ms` (0x103f4d2ca), `photos_throttled_backoff_limit_ms` (0x103f4d30a), `photos_error_list_retry_interval_ms` (0x103f4d557), `photos_catalog_sync_time_interval_ms` (0x103f4cf15) |
| Rebuild | the Google Photos copy passes `--min-age 300s`, so files changed in the last 5 minutes wait; every change in a Photos-backed folder schedules another pass 5 minutes after it (a newer change pushes it back), which picks them up. Backing up to Drive still starts at once (upload_idle_delay_msec = 0) | src/engine.rs PHOTOS_SETTLE_SECS, photos_settle, backup_folder; test `change_list_backs_off_like_the_original` |

Still estimated after this pass: the 120 s mirror / 300 s folder re-check used only when the file watcher or Drive's change list fails (the original relies on macOS file events and has no matching setting), the ransomware thresholds (the original's detection runs on Google's servers), and the rebuild's own badge timings.

## Google Docs, Sheets and Slides in the mirrored folder
| Rule | Value | Source |
|---|---|---|
| Google documents appear as link files | `.gdoc`, `.gsheet`, `.gslides` are the ones a person may create by name | flag default `allowed_new_gdoc_file_name_pattern` (0x103f4c2e6) = `.*Google Docs.*\.gdoc|.*Google Sheets.*\.gsheet|.*Google Slides.*\.gslides` |
| Every link extension and its document type | gdoc 2, gslides 3, gform 4, gsheet 5, gdraw 6, gtable 7, gmap 8, gscript 9, gsite 10, gmaillayout 11, gnote 12, glink 13, gjam 14, gvid 15, gprj 16, gpic 17 (16 extensions) | function 0x100d541bd compares the extension with each name in turn and reads the type from the table at 0x1042a7740 (pairs of type number and pointer to the name, e.g. 5 → "gsheet" @0x1040ebf78, 3 → "gslides" @0x1040ebfac) |
| Link file contents | JSON with the warning "WARNING! DO NOT EDIT THIS FILE! ANY CHANGES MADE WILL BE LOST!" and `doc_id` (rebuild adds `resource_key`, `email`, `url`) | the warning is entry 0x102bf5895 of the translated-message table (looked up by number, so no code points at it directly); `doc_id` field names |
| New Google Docs/Sheets/Slides from the file menu (original only) | menu actions 12, 13, 14 pass "gdoc", "gsheet", "gslides" to a queued create task; encrypted variants 21–23 use "gcsedoc", "gcsesheet", "gcseslides"; create pages `https://docs.google.com/{document,spreadsheets,presentation}/create` | function 0x100cef4c0 (`ExecuteContextMenuAction`, context_menu_util.cc), function 0x100cf2120; strings 0x1040ebf50–0x1040ebfc0. **Not built** in the rebuild yet |
| Rebuild | each mirror pass lists Drive with `--drive-show-all-gdocs`, so every document comes with its own Google type (`application/vnd.google-apps.spreadsheet` → `.gsheet`, `form` → `.gform`, `map` → `.gmap`…, the original's 16 extensions; shortcuts are left out); writes a link file per document (renamed/moved/deleted documents update or remove it; only files carrying the warning are removed), never uploads link files, and opens them with `drive-linux --open-doc <file>` registered for all 16 file types. Docs, Sheets, Slides, Forms, Drawings and Apps Script open in their editor; the rest open through `https://drive.google.com/open?id=<id>` (**inferred**: the original keeps an `alternate_link` field, 0x1040c43e1, but where it opens wasn't traced) | src/gdocs.rs KINDS, parse_docs, url; src/engine.rs mirror_pair; src/settings.rs install_launcher; packaging/drive-linux-mime.xml, drive-linux-open-doc.desktop; tests `google_documents_become_link_files`, `packaged_file_types_cover_every_link_extension` |

## App icon, taskbar/Dock presence and file type names
| Rule | Value | Source |
|---|---|---|
| The app normally has no Dock (taskbar) entry: it lives in the menu bar (tray) | `LSUIElement = true` | Info.plist |
| A Dock entry appears while at least one of its windows is open and goes away when the last one closes | open windows are kept in a set; the first one added calls `setActivationPolicy:` with Regular (0), removing the last calls it with Accessory (1) | function 0x1003aa820 (`cello::fs::DialogManagerImpl`, method table slot 3 at 0x104249148) → function 0x1001232f0 (`[NSApp setActivationPolicy: show ^ 1]`, selector slot 0x10437d788) |
| Rebuild | closing the window (when "keep running" is on) hides it completely, so it leaves the taskbar; opening it from the tray brings the entry back. A minimized window keeps its entry, as a minimized window keeps the Dock icon. The window carries the class ("drive-linux", "drive-linux") so Plasma ties it to drive-linux.desktop and shows the app's logo, and the menu entries name the logo by its full path, so an icon theme's generic "drive" icon can't stand in for it | src/app.rs set_window_class (also retried on the first frames), close handling in `ui`; src/settings.rs icon_path, install_launcher |
| Name of each link file type | gdoc "Google Document", gsheet "Google Sheets", gslides "Google Presentation", gform "Google Form", gdraw "Google Drawing", glink "Google Drive Link", gmap "Google Map", gsite "Google Site", gmaillayout "Email layout", gnote "Google Note", gtable "Google Table", gscript "Google Script", gjam "Google Jam", gvid "Google Vids", gprj "Google Project", gpic "Google Pics" (each with its own icon, e.g. gsheet.icns) | Info.plist `CFBundleDocumentTypes` (`CFBundleTypeName`, role Editor); the original also declares `gcse` ("Google CSE") and `gdrive` ("Google GDrive"), which no code path above produces |
| Rebuild | the same English names in the file types file managers show (Vietnamese names kept); the theme's office icons stand in for Google's per-type icons | packaging/drive-linux-mime.xml |
