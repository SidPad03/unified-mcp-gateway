import SwiftUI

/// The menu-bar popover: status, the two actions people actually reach for, and
/// a way back into the window.
struct MenuBarView: View {
    @Environment(AgentModel.self) private var model
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            status
            Divider().padding(.vertical, 6)
            actions
            Divider().padding(.vertical, 6)
            footer
        }
        .padding(12)
        .frame(width: 268)
    }

    private var status: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                StatusDot(
                    tint: model.connection?.state.tint ?? Palette.text3,
                    pulsing: model.connection?.state == .reconnecting
                )
                Text(model.connection?.state.label ?? "Starting")
                    .font(.system(size: Typo.small, weight: .semibold))
                Spacer(minLength: 0)
            }
            Text(model.connection?.agentId ?? "—")
                .font(.system(size: Typo.caption, design: .monospaced))
                .foregroundStyle(Palette.text3)
                .lineLimit(1)
                .truncationMode(.middle)

            if model.isSignedIn {
                HStack(spacing: 14) {
                    Metric(
                        value: "\(model.stats?.toolsRegistered ?? 0)",
                        label: "tools"
                    )
                    Metric(
                        value: "\(model.stats?.backendsReady ?? 0)/\(model.stats?.backendsTotal ?? 0)",
                        label: "backends"
                    )
                    Metric(
                        value: Format.count(model.stats?.callsTotal ?? 0),
                        label: "calls"
                    )
                }
                .padding(.top, 2)
            }
        }
    }

    private var actions: some View {
        VStack(spacing: 2) {
            MenuRow(title: "Open MCP Gateway Agent", icon: "macwindow") {
                NSApp.setActivationPolicy(.regular)
                openWindow(id: "main")
                NSApp.activate(ignoringOtherApps: true)
            }
            MenuRow(title: "Reconnect", icon: "arrow.clockwise") {
                Task { await model.reconnect() }
            }
            MenuRow(title: "Open Dashboard", icon: "arrow.up.right.square") {
                model.openDashboard()
            }
            .disabled(model.apiBaseURL == nil)
            MenuRow(title: "Check for Updates…", icon: "arrow.down.circle") {
                Task { await model.updater.check() }
            }
        }
    }

    private var footer: some View {
        VStack(spacing: 2) {
            if case let .available(release) = model.updater.state {
                HStack(spacing: 6) {
                    Image(systemName: "sparkles").font(.system(size: Typo.micro))
                    Text("Version \(release.version) is available")
                        .font(.system(size: Typo.caption))
                    Spacer(minLength: 0)
                }
                .foregroundStyle(Palette.beam)
                .padding(.horizontal, 7)
                .padding(.vertical, 5)
            }
            SettingsLink {
                HStack(spacing: 8) {
                    Image(systemName: "gearshape").frame(width: 15)
                    Text("Settings…").font(.system(size: Typo.small))
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 7)
                .padding(.vertical, 5)
                .contentShape(.rect)
            }
            .buttonStyle(.plain)

            MenuRow(title: "Quit", icon: "power") {
                NSApp.terminate(nil)
            }
        }
    }
}

private struct Metric: View {
    let value: String
    let label: String

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(value)
                .font(.system(size: Typo.body, weight: .semibold, design: .rounded))
            Text(label)
                .font(.system(size: Typo.micro))
                .foregroundStyle(Palette.text3)
        }
    }
}

private struct MenuRow: View {
    let title: String
    let icon: String
    let action: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: icon).frame(width: 15)
                Text(title).font(.system(size: Typo.small))
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 7)
            .padding(.vertical, 5)
            .background(
                hovering ? Palette.beam.opacity(0.16) : .clear,
                in: .rect(cornerRadius: 6)
            )
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
    }
}
