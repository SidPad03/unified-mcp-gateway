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

    /// No agent id under the status line. It carried this Mac's own hostname —
    /// the one fact the person looking at their own menu bar already has — and
    /// it was the widest thing in the popover. The same call the sidebar made
    /// about its subtitle.
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

            if model.isSignedIn {
                // Space-between, not equal thirds. Equal columns left every
                // metric hugging its column's left edge, which read as three
                // figures drifting toward the left of the popover with a dead
                // right margin; spacers between them spread the row edge to
                // edge with even gaps.
                HStack(alignment: .top, spacing: 0) {
                    Metric(
                        value: "\(model.stats?.toolsRegistered ?? 0)",
                        label: "tools"
                    )
                    Spacer(minLength: 16)
                    Metric(
                        value: "\(model.stats?.backendsReady ?? 0)/\(model.stats?.backendsTotal ?? 0)",
                        label: "backends"
                    )
                    Spacer(minLength: 16)
                    Metric(
                        value: Format.count(model.stats?.callsTotal ?? 0),
                        label: "calls"
                    )
                }
                .padding(.top, 2)
                .padding(.trailing, 6)
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
            updateRow
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

    /// The update affordance, mirroring the sidebar's footer row: a green
    /// action while there is something to do, a progress line while it
    /// downloads, and "Update now" once a staged bundle is waiting on a
    /// restart the user declined.
    @ViewBuilder
    private var updateRow: some View {
        switch model.updater.state {
        case let .available(release):
            MenuRow(
                title: "Update to version \(release.version)",
                icon: "arrow.down.circle.fill",
                tint: Palette.beam
            ) {
                Task { await model.updater.requestUpdate(release) }
            }

        case .downloading:
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Downloading update…")
                    .font(.system(size: Typo.small))
                    .foregroundStyle(Palette.text3)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 7)
            .padding(.vertical, 5)

        case let .readyToInstall(release):
            MenuRow(title: "Update now", icon: "arrow.down.circle.fill", tint: Palette.beam) {
                Task { await model.updater.requestUpdate(release) }
            }

        case .readyToRelaunch:
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Relaunching…")
                    .font(.system(size: Typo.small))
                    .foregroundStyle(Palette.text3)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 7)
            .padding(.vertical, 5)

        case .idle, .checking, .upToDate, .failed:
            EmptyView()
        }
    }
}

private struct Metric: View {
    let value: String
    let label: String

    var body: some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(value)
                .font(.system(size: Typo.body, weight: .semibold, design: .rounded))
                .monospacedDigit()
            Text(label)
                .font(.system(size: Typo.micro))
                .foregroundStyle(Palette.text3)
        }
    }
}

private struct MenuRow: View {
    let title: String
    let icon: String
    var tint: Color?
    let action: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: icon).frame(width: 15)
                Text(title).font(.system(size: Typo.small))
                Spacer(minLength: 0)
            }
            .foregroundStyle(tint ?? Color.primary)
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
