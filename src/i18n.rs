//! English is the default (as in the original). Vietnamese uses the original's own
//! Vietnamese wording where the program has it.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static VI: AtomicBool = AtomicBool::new(false);

pub fn set_vietnamese(on: bool) {
    VI.store(on, Ordering::Relaxed);
}

pub fn is_vi() -> bool {
    VI.load(Ordering::Relaxed)
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| {
        [
            ("Home", "Trang chủ"),
            ("Sync activity", "Hoạt động đồng bộ hóa"),
            ("Notifications", "Thông báo"),
            ("Connected folders", "Thư mục đã kết nối"),
            ("Offline files", "Tệp ngoại tuyến"),
            ("Shared with me", "Được chia sẻ với tôi"),
            ("Settings", "Cài đặt"),
            ("Search in Drive", "Tìm trong Drive"),
            ("Sync status", "Trạng thái đồng bộ hóa"),
            ("Up to date", "Đã cập nhật"),
            ("Paused", "Đã tạm dừng"),
            ("Offline", "Ngoại tuyến"),
            ("Your files are up to date", "Tệp của bạn đã được cập nhật"),
            ("Everything is up to date", "Mọi thứ đều cập nhật"),
            ("Fetching new changes", "Tìm nạp thay đổi mới"),
            ("Loading your files", "Đang tải tệp của bạn"),
            ("You are signed out", "Bạn đã đăng xuất"),
            ("Syncing items", "Đang đồng bộ hóa các mục"),
            ("Pause syncing", "Tạm dừng đồng bộ hóa"),
            ("Resume syncing", "Tiếp tục đồng bộ hóa"),
            ("View all sync activity", "Xem tất cả hoạt động đồng bộ hóa"),
            ("Quick links", "Đường liên kết nhanh"),
            ("Open Drive folder", "Mở thư mục Drive"),
            ("Open Drive web", "Mở trang web Drive"),
            ("Add more folders to sync", "Thêm thư mục để đồng bộ hóa"),
            ("View all", "Xem tất cả"),
            ("View all notifications", "Xem tất cả thông báo"),
            ("No new notifications", "Không có thông báo mới"),
            ("Notifications will show up here", "Thông báo sẽ xuất hiện tại đây"),
            ("Activity on your files and folders will show up here", "Hoạt động trên các tệp và thư mục của bạn sẽ xuất hiện ở đây"),
            ("Storage usage", "Mức sử dụng bộ nhớ"),
            ("Manage storage", "Quản lý bộ nhớ"),
            ("Folders connected to Drive", "Thư mục đã kết nối với Drive"),
            ("Connect a folder", "Kết nối một thư mục"),
            ("Files available offline", "Tệp có thể dùng khi không có mạng"),
            ("Files made offline will appear here", "Tệp được đặt ở chế độ ngoại tuyến sẽ xuất hiện ở đây"),
            ("Nothing has been shared with you yet", "Chưa có nội dung nào được chia sẻ với bạn"),
            ("Name", "Tên"),
            ("File size", "Kích thước tệp"),
            ("Status", "Trạng thái"),
            ("Size", "Kích thước"),
            ("Date shared", "Ngày chia sẻ"),
            ("Synced", "Đã đồng bộ hóa"),
            ("Moved to trash", "Đã chuyển vào thùng rác"),
            ("Upload will begin shortly", "Sắp bắt đầu tải lên"),
            ("Sign in to Google Drive", "Đăng nhập vào Google Drive"),
            ("Sign in with browser", "Đăng nhập bằng trình duyệt"),
            ("You’ll be directed to your browser to finish signing into your account", "Bạn sẽ được chuyển đến trình duyệt để hoàn tất việc đăng nhập tài khoản"),
            ("Waiting for you to finish signing in in the browser…", "Đang chờ bạn đăng nhập trong trình duyệt…"),
            ("Preferences", "Lựa chọn ưu tiên"),
            ("Error list", "Danh sách lỗi"),
            ("Help", "Trợ giúp"),
            ("Send feedback", "Gửi ý kiến phản hồi"),
            ("About", "Giới thiệu"),
            ("Version", "Phiên bản"),
            ("Quit", "Thoát"),
            ("Quit Google Drive", "Thoát Google Drive"),
            ("Disconnect account", "Ngắt kết nối tài khoản"),
            ("Disconnect account?", "Ngắt kết nối tài khoản?"),
            ("Disconnect", "Ngắt kết nối"),
            ("Cancel", "Hủy"),
            ("Close", "Đóng"),
            ("Google Account", "Tài khoản Google"),
            ("General settings", "Cài đặt chung"),
            ("Account settings", "Cài đặt tài khoản"),
            ("Appearance", "Giao diện"),
            ("Light", "Sáng"),
            ("Dark", "Tối"),
            ("Language", "Ngôn ngữ"),
            ("Launch on login", "Chạy khi đăng nhập"),
            ("Launch Google Drive when you login to your computer", "Chạy Google Drive khi bạn đăng nhập vào máy tính"),
            ("Proxy settings", "Cài đặt proxy"),
            ("Auto-detect", "Tự động phát hiện"),
            ("Direct connection", "Kết nối trực tiếp"),
            ("Bandwidth settings", "Cài đặt băng thông"),
            ("Download rate", "Tốc độ tải xuống"),
            ("Upload rate", "Tốc độ tải lên"),
            ("kB/s", "kB/giây"),
            ("My Drive syncing options", "Cách đồng bộ hóa Drive của tôi"),
            ("Stream files", "Truy cập trực tuyến tệp"),
            ("Store all My Drive files in the cloud only", "Chỉ lưu trữ tất cả tệp Drive của tôi trên đám mây"),
            ("Access files from a folder on your computer", "Truy cập các tệp thông qua một thư mục trên máy tính"),
            ("Streaming uses less hard drive space.", "Khi truyền trực tuyến, bạn sẽ sử dụng ít dung lượng ổ đĩa cứng hơn."),
            ("Mirror files", "Phản chiếu tệp"),
            ("Store all My Drive files in the cloud and on your computer", "Lưu trữ tất cả tệp Drive của tôi trên đám mây và trên máy tính"),
            ("All files are automatically available offline", "Tất cả tệp đều tự động ở chế độ có thể sử dụng khi không có mạng"),
            ("Google Drive streaming location", "Vị trí truy cập trực tuyến vào Google Drive"),
            ("My Drive folder location", "Vị trí thư mục Drive của tôi"),
            ("Change folder location", "Thay đổi vị trí thư mục"),
            ("Open folder", "Mở thư mục"),
            ("Local cached files directory", "Thư mục lưu trữ nội dung cục bộ của tệp"),
            ("Change", "Thay đổi"),
            ("Cache settings", "Cài đặt bộ nhớ đệm"),
            ("Limit the amount of storage on your computer used by cached files. Limit won’t apply to files that need to sync or files you make available offline.", "Giới hạn mức dung lượng lưu trữ mà các tệp được lưu vào bộ nhớ đệm có thể sử dụng trên máy tính. Giới hạn này không áp dụng cho những tệp cần đồng bộ hóa hoặc tệp bạn đặt ở chế độ có thể dùng khi không có mạng."),
            ("Limit cache size", "Giới hạn kích thước bộ nhớ đệm"),
            ("Some changes require Google Drive to restart", "Một số thay đổi yêu cầu khởi động lại Google Drive"),
            ("Restart now", "Khởi động lại ngay"),
            ("Sync with Google Drive", "Đồng bộ hóa với Google Drive"),
            ("Upload all files and folders, including photos and videos. If you edit or remove files, those changes will be automatically synced.", "Tải tất cả tệp và thư mục, bao gồm cả ảnh và video lên. Nếu bạn chỉnh sửa hoặc xóa tệp, thì những thay đổi đó sẽ tự động đồng bộ hóa."),
            ("Back up to Google Photos", "Sao lưu vào Google Photos"),
            ("Upload only photos and videos. If you remove files, those changes won’t be automatically synced.", "Chỉ tải ảnh và video lên. Nếu bạn xóa tệp, những thay đổi đó sẽ không tự động đồng bộ hóa."),
            ("Safely back up your files", "Sao lưu an toàn tệp của bạn"),
            ("Upload, store, and sync your files to Google Drive and Google Photos. Choose a folder on your computer to get started.", "Tải lên, lưu trữ và đồng bộ hóa các tệp của bạn với Google Drive và Google Photos. Chọn một thư mục trên máy tính của bạn để bắt đầu."),
            ("Stop syncing", "Dừng đồng bộ hóa"),
            ("Stop backing up", "Dừng sao lưu"),
            ("Stop syncing and backing up", "Dừng đồng bộ hóa và sao lưu"),
            ("Connect Google Photos", "Kết nối Google Photos"),
            ("Open Google Drive", "Mở Google Drive"),
            ("Dismiss", "Bỏ qua"),
            ("Shared drives", "Bộ nhớ dùng chung"),
            ("Shared drive syncing", "Đồng bộ hóa bộ nhớ dùng chung"),
            ("Loading shared drives", "Đang tải bộ nhớ dùng chung"),
            ("This account has no shared drives.", "Tài khoản này chưa có bộ nhớ dùng chung."),
            ("All shared drives are streamed inside “Shared drives”:", "Mọi bộ nhớ dùng chung được truy cập trực tuyến trong “Shared drives”:"),
            ("Shared drives switched on are mirrored to", "Bộ nhớ dùng chung được bật sẽ được phản chiếu vào"),
            ("Toggle shared drive", "Bật/tắt bộ nhớ dùng chung"),
            ("Shared drive syncing enabled", "Đã bật đồng bộ hóa bộ nhớ dùng chung"),
            ("Shared drive syncing disabled", "Đã tắt đồng bộ hóa bộ nhớ dùng chung"),
            ("Shared drive sync settings updated", "Đã cập nhật cài đặt đồng bộ hóa bộ nhớ dùng chung"),
            ("Can’t complete action", "Không thể hoàn tất thao tác"),
            ("isn’t in a Google Drive folder.", "không nằm trong thư mục Google Drive."),
            ("Available offline", "Có thể dùng khi không có mạng"),
            ("Online only", "Chỉ trực tuyến"),
            ("Link copied to the clipboard.", "Đã sao chép đường liên kết vào bộ nhớ đệm."),
            ("Show Google Drive in the file manager menu", "Hiện Google Drive trong menu chuột phải của trình quản lý tệp"),
            ("Quit now", "Thoát ngay"),
            ("Finish syncing, then quit", "Đồng bộ hóa xong rồi thoát"),
            ("Finishing syncing, then Google Drive will quit…", "Đang hoàn tất đồng bộ hóa, sau đó Google Drive sẽ thoát…"),
            ("Keep running in the system tray when the window is closed", "Tiếp tục chạy ở khay hệ thống khi đóng cửa sổ"),
            ("No system tray was found on this desktop.", "Không tìm thấy khay hệ thống trên màn hình này."),
            ("Don’t show this again", "Không hiện lại thông báo này"),
            ("Quit Google Drive?", "Thoát khỏi Google Drive?"),
            ("Files are still syncing. Your changes won’t sync unless Google Drive is running.", "Các tệp vẫn đang đồng bộ hóa. Những thay đổi của bạn sẽ không đồng bộ hóa trừ trường hợp Google Drive đang chạy."),
            ("Add another account", "Thêm một tài khoản khác"),
            ("Disconnect an account to add another", "Ngắt kết nối một tài khoản để thêm tài khoản khác"),
            ("No matching results", "Không có kết quả phù hợp"),
            ("Search in Drive web", "Tìm trên trang web Drive"),
            ("Add", "Thêm"),
            ("Choose to upload files in this device to Google Drive or Google Photos", "Chọn tải các tệp trong thiết bị này lên Google Drive hoặc Google Photos"),
            ("Remember my choice for this device", "Ghi nhớ lựa chọn của tôi cho thiết bị này"),
            ("Add device", "Thêm thiết bị"),
            ("Not now", "Để sau"),
            ("Add folder", "Thêm thư mục"),
            ("Remove", "Xóa"),
            ("Add new files to be available offline", "Thêm tệp mới để có thể dùng khi không có mạng"),
            ("Making files available offline…", "Đang đặt tệp ở chế độ dùng khi không có mạng…"),
            ("Account:", "Tài khoản:"),
            ("USB devices backed up when connected", "Thiết bị USB được sao lưu khi kết nối"),
            ("Network-attached storage backed up every 6 hours", "Ổ lưu trữ mạng được sao lưu 6 giờ một lần"),
            ("USB and External Devices", "USB và thiết bị bên ngoài"),
            ("Prompt me to back up devices", "Nhắc tôi sao lưu thiết bị"),
            ("Ignored USB Devices", "Thiết bị USB bị bỏ qua"),
            ("You won’t be prompted to sync or back up these devices", "Bạn sẽ không được nhắc đồng bộ hóa hoặc sao lưu các thiết bị này"),
            ("Ignored USB devices will show up here", "Thiết bị USB bị bỏ qua sẽ hiện ở đây"),
            ("Remove ignored device", "Bỏ thiết bị khỏi danh sách bỏ qua"),
            ("Stop ignoring all", "Ngừng bỏ qua tất cả"),
            ("A newer rclone is needed for Google Photos (Google changed its access in March 2025). It is downloaded from rclone.org, checked against its published checksum, and kept in this app's own folder.", "Cần rclone mới hơn để dùng Google Photos (Google đổi cách truy cập từ tháng 3/2025). Bản mới được tải từ rclone.org, kiểm tra mã băm công bố và lưu trong thư mục riêng của ứng dụng."),
            ("Downloading…", "Đang tải xuống…"),
            ("Get the latest rclone", "Tải rclone mới nhất"),
            ("Inside it: “My Drive” and “Shared drives” (no shared drives on this account).", "Bên trong: “Drive của tôi” và “Bộ nhớ dùng chung” (tài khoản này chưa có bộ nhớ dùng chung)."),
            ("Inside it: “My Drive” and “Shared drives”:", "Bên trong: “Drive của tôi” và “Bộ nhớ dùng chung”:"),
            ("Upload RAW files", "Tải tệp RAW lên"),
            ("The selected folder must be empty.", "Thư mục đã chọn phải trống."),
            ("The selected location is not a folder.", "Vị trí đã chọn không phải là thư mục."),
            ("Google Photos needs to be connected first.", "Cần kết nối Google Photos trước."),
            ("Google may block this sign-in: since March 2025 it only allows newer apps to add photos. Use “Sync with Google Drive” to keep photos safe in Drive instead.", "Google có thể chặn lần đăng nhập này: từ tháng 3/2025 Google chỉ cho ứng dụng mới hơn thêm ảnh. Hãy dùng “Đồng bộ hóa với Google Drive” để giữ ảnh an toàn trong Drive."),
            ("Open in web", "Mở trên web"),
            ("Copy link", "Sao chép đường liên kết"),
            ("Refresh", "Làm mới"),
            ("Add files", "Thêm tệp"),
            ("Needs my attention", "Cần tôi chú ý"),
            ("Signed in as", "Đã đăng nhập:"),
            ("Not connected", "Chưa kết nối"),
            ("rclone is not installed. Open a terminal and run: sudo apt install rclone", "Chưa cài rclone. Mở Terminal và chạy: sudo apt install rclone"),
            ("Folders are synced to Google Drive under", "Thư mục được đồng bộ hóa vào Google Drive, trong"),
            ("This folder must be empty.", "Thư mục này phải trống."),
            ("Files you open while streaming are kept in the local cache.", "Tệp bạn mở khi truy cập trực tuyến được giữ trong bộ nhớ đệm cục bộ."),
            ("In mirror mode every file is already on this computer.", "Ở chế độ phản chiếu, mọi tệp đã có trên máy tính này."),
            ("Loading items...", "Đang tải các mục..."),
            ("Just now", "Vừa xong"),
            ("Shared drive", "Bộ nhớ dùng chung"),
            ("Folder", "Thư mục"),
            ("Generate diagnostic info", "Tạo thông tin chẩn đoán"),
            ("Enable verbose logging", "Bật tính năng ghi nhật ký chi tiết"),
            ("Disable verbose logging", "Tắt chế độ Ghi nhật ký chi tiết"),
            ("Generating diagnostic info", "Đang tạo thông tin chẩn đoán"),
            ("Diagnostic info is being generated. This may take a few minutes. Please wait…", "Đang tạo thông tin chẩn đoán. Quá trình này có thể mất vài phút. Vui lòng chờ…"),
            ("Diagnostic info created", "Đã tạo thông tin chẩn đoán"),
            ("Diagnostic info could not be created", "Không thể tạo thông tin chẩn đoán"),
            ("OK", "OK"),
            ("Add shortcut to Google Drive", "Thêm lối tắt vào Google Drive"),
            ("Take action", "Tiến hành xử lý"),
            ("I recognize it", "Tôi nhận ra hoạt động này"),
            ("Allow sync", "Cho phép đồng bộ hoá"),
            ("Clear files and resume syncing", "Xoá các tệp và tiếp tục đồng bộ hoá"),
            ("Independent Linux app. Connects to Google Drive through rclone; not made by Google.", "Ứng dụng độc lập cho Linux. Kết nối với Google Drive thông qua rclone; không phải sản phẩm của Google."),
        ]
        .into_iter()
        .collect()
    })
}

/// Translate an English UI string (returned unchanged in English mode).
pub fn tr(en: &'static str) -> &'static str {
    if is_vi() {
        table().get(en).copied().unwrap_or(en)
    } else {
        en
    }
}

#[cfg(test)]
mod tests {
    /// Every text the app shows through tr() has a Vietnamese translation, and no English text
    /// is listed twice (the second one would silently win).
    #[test]
    fn every_shown_text_is_translated_once() {
        let pair = |s: &str| -> Vec<String> {
            let mut out = Vec::new();
            let mut rest = s;
            while let Some(i) = rest.find("(\"") {
                rest = &rest[i + 2..];
                let Some(end) = rest.find("\", \"") else { break };
                out.push(rest[..end].to_string());
                rest = &rest[end..];
            }
            out
        };
        let table_src = include_str!("i18n.rs");
        let table_src = &table_src[..table_src.find("#[cfg(test)]").unwrap()];
        let keys = pair(table_src);
        let mut seen = std::collections::HashSet::new();
        for k in &keys {
            assert!(seen.insert(k.clone()), "listed twice: {k}");
        }
        assert!(keys.len() > 150, "the table was read ({} entries)", keys.len());
        let mut checked = 0;
        for src in [include_str!("app.rs"), include_str!("engine.rs"), include_str!("filemenu.rs"), include_str!("tray.rs")] {
            let mut rest = src;
            while let Some(i) = rest.find("tr(\"") {
                rest = &rest[i + 4..];
                let end = rest.find("\")").unwrap();
                let text = &rest[..end];
                if !text.contains('"') {
                    assert!(seen.contains(text), "no Vietnamese for: {text}");
                    checked += 1;
                }
                rest = &rest[end..];
            }
        }
        assert!(checked > 150, "texts checked: {checked}");
    }
}
