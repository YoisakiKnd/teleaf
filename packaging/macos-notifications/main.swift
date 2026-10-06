// Runs only while applying a bounded notification batch. No Telegram data access.
import AppKit
import CoreServices
import Foundation
import UserNotifications

func finish(_ error: String? = nil) -> Never {
    let reply: [String: Any] = error.map { ["error": $0] } ?? ["ok": true]
    let data = try! JSONSerialization.data(withJSONObject: reply)
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([10]))
    exit(error == nil ? 0 : 1)
}

if CommandLine.arguments.dropFirst().first == "--self-test" {
    // No center access, permission prompt or notification. Check native request serialization.
    let content = UNMutableNotificationContent()
    content.title = "群 <&>"
    content.body = "$(not-code) </text>"
    let request = UNNotificationRequest(identifier: "test", content: content, trigger: nil)
    assert(request.content.title == "群 <&>" && request.content.body == "$(not-code) </text>")
    assert(request.content.sound == nil && Bundle.main.bundleIdentifier == "org.teleaf.notifications")
    finish()
}

if CommandLine.arguments.dropFirst().first == "--probe" {
    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)
    guard LSRegisterURL(Bundle.main.bundleURL as CFURL, true) == noErr else {
        finish("无法注册通知助手")
    }
    UNUserNotificationCenter.current().getNotificationSettings { _ in finish() }
    DispatchQueue.main.asyncAfter(deadline: .now() + 8) { finish("系统通知服务响应超时") }
    app.run()
    exit(1)
}

var input = Data()
while input.count <= 524288 {
    let chunk = FileHandle.standardInput.readData(ofLength: min(8192, 524289 - input.count))
    if chunk.isEmpty { break }
    input.append(chunk)
}
guard input.count <= 524288,
      let payload = try? JSONSerialization.jsonObject(with: input) as? [String: Any],
      let account = payload["account"] as? String,
      !account.isEmpty, account.count <= 32,
      account.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-") }),
      let commands = payload["commands"] as? [[String: Any]], commands.count <= 16 else {
    finish("无效的通知请求")
}
let prefix = "teleaf.\(account)."
for command in commands {
    guard let op = command["op"] as? String, ["show", "remove", "clear"].contains(op) else {
        finish("未知通知操作")
    }
    if op != "clear" {
        guard let group = command["group"] as? NSNumber, group.int64Value > 0 else {
            finish("无效的通知分组")
        }
    }
    if op == "show" {
        guard let title = command["title"] as? String, title.utf8.count <= 8192,
              let body = command["body"] as? String, body.utf8.count <= 8192,
              command["silent"] is Bool else { finish("无效的通知内容") }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
// Register the bundled identity so System Settings attributes alerts to Teleaf.
guard LSRegisterURL(Bundle.main.bundleURL as CFURL, true) == noErr else {
    finish("无法注册 Teleaf 通知助手")
}
let center = UNUserNotificationCenter.current()
final class Delegate: NSObject, UNUserNotificationCenterDelegate {
    func userNotificationCenter(_ center: UNUserNotificationCenter,
                                willPresent notification: UNNotification,
                                withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler(notification.request.content.sound == nil ? [.banner, .list] : [.banner, .list, .sound])
    }
}
let delegate = Delegate()
center.delegate = delegate

func apply(_ index: Int) {
    if index == commands.count { finish() }
    let command = commands[index]
    let op = command["op"] as! String
    func next() { DispatchQueue.main.async { apply(index + 1) } }
    if op == "clear" {
        center.getDeliveredNotifications { notifications in
            center.removeDeliveredNotifications(withIdentifiers: notifications.map { $0.request.identifier }.filter { $0.hasPrefix(prefix) })
            center.getPendingNotificationRequests { requests in
                center.removePendingNotificationRequests(withIdentifiers: requests.map { $0.identifier }.filter { $0.hasPrefix(prefix) })
                next()
            }
        }
        return
    }
    let id = prefix + (command["group"] as! NSNumber).stringValue
    center.removePendingNotificationRequests(withIdentifiers: [id])
    center.removeDeliveredNotifications(withIdentifiers: [id])
    if op == "remove" { next(); return }
    let content = UNMutableNotificationContent()
    content.title = command["title"] as! String
    content.body = command["body"] as! String
    content.threadIdentifier = id
    if !(command["silent"] as! Bool) { content.sound = .default }
    center.add(UNNotificationRequest(identifier: id, content: content, trigger: nil)) { error in
        if let error = error { finish("通知提交失败：\(error.localizedDescription)") }
        next()
    }
}

if commands.contains(where: { $0["op"] as? String == "show" }) {
    center.getNotificationSettings { settings in
        if settings.authorizationStatus == .notDetermined {
            center.requestAuthorization(options: [.alert, .sound]) { granted, error in
                guard granted else { finish("请在系统设置 → 通知中允许 Teleaf Notifications：\(error?.localizedDescription ?? "通知权限未授予")") }
                DispatchQueue.main.async { apply(0) }
            }
        } else if settings.authorizationStatus == .denied {
            finish("请在系统设置 → 通知中允许 Teleaf Notifications")
        } else { DispatchQueue.main.async { apply(0) } }
    }
} else { apply(0) }
// Parent also enforces a deadline; never leave an orphan helper after a stalled daemon.
DispatchQueue.main.asyncAfter(deadline: .now() + 60) { finish("通知服务响应超时") }
app.run()
