import AppKit
import SwiftUI

/// The Settings window — the standard ⌘, one, reached from the app menu or the
/// menu-bar popover.
///
/// Everything configurable lives here rather than on a page inside the main
/// window: on a Mac, "where do I change the server address" has one answer, and
/// it is this window.
struct PreferencesView: View {
    var body: some View {
        TabView {
            GatewayPane()
                .tabItem { Label("Gateway", systemImage: "network") }
            GeneralPane()
                .tabItem { Label("General", systemImage: "gearshape") }
            UpdatesPane()
                .tabItem { Label("Updates", systemImage: "arrow.down.circle") }
            AboutPane()
                .tabItem { Label("About", systemImage: "info.circle") }
        }
        .frame(width: 520, height: 420)
    }
}

// ── Gateway ─────────────────────────────────────────────────────────────

private struct GatewayPane: View {
    @Environment(AgentModel.self) private var model

    @State private var gatewayUrl = ""
    @State private var agentId = ""
    @State private var dashboardUrl = ""
    @State private var tlsSkipVerify = false
    @State private var loaded = false
    @State private var confirmingSignOut = false

    var body: some View {
        Form {
            Section("Account") {
                if let account = model.account {
                    LabeledContent("Signed in as") {
                        HStack(spacing: 7) {
                            Image(systemName: "person.crop.circle.fill")
                                .foregroundStyle(Palette.beam)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(account.username).font(.system(size: Typo.small, weight: .medium))
                                Text(account.isOwner ? "Owner" : "Standard user")
                                    .font(.system(size: Typo.micro))
                                    .foregroundStyle(Palette.text3)
                            }
                        }
                    }
                    LabeledContent("API key") {
                        // Stored in the Keychain. The app shows that one exists
                        // and never shows the value.
                        Text(model.config?.hasApiKey == true ? "Stored in your Keychain" : "Missing")
                            .font(.system(size: Typo.caption))
                            .foregroundStyle(
                                model.config?.hasApiKey == true
                                    ? Palette.text3 : Palette.deny)
                    }
                    Button("Sign out…", role: .destructive) { confirmingSignOut = true }
                } else {
                    Text("Not signed in.")
                        .foregroundStyle(Palette.text3)
                }
            }

            Section("Connection") {
                TextField("Gateway address", text: $gatewayUrl)
                    .font(.system(size: Typo.small, design: .monospaced))
                TextField("Machine name", text: $agentId)
                    .font(.system(size: Typo.small, design: .monospaced))
                TextField("Dashboard address (optional)", text: $dashboardUrl)
                    .font(.system(size: Typo.small, design: .monospaced))
                Toggle("Skip TLS certificate verification", isOn: $tlsSkipVerify)
                if tlsSkipVerify {
                    Text(
                        "The identity of the gateway is not checked. Use this only for a "
                            + "self-signed certificate on a network you trust."
                    )
                    .font(.system(size: Typo.caption))
                    .foregroundStyle(Palette.warn)
                }
                HStack {
                    Spacer()
                    Button("Apply and reconnect") { apply() }
                        .buttonStyle(.glassProminent)
                }
            }
        }
        .formStyle(.grouped)
        .onAppear(perform: load)
        .alert("Sign out of the gateway?", isPresented: $confirmingSignOut) {
            Button("Sign out", role: .destructive) { Task { await model.signOut() } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(
                "This Mac disconnects and its tools are withdrawn from the gateway. "
                    + "The API key is removed from your Keychain; the key itself stays valid "
                    + "until you delete it in the dashboard."
            )
        }
    }

    private func load() {
        guard !loaded, let config = model.config else { return }
        gatewayUrl = config.gatewayUrl
        agentId = config.agentId
        dashboardUrl = config.dashboardUrl ?? ""
        tlsSkipVerify = config.tlsSkipVerify
        loaded = true
    }

    private func apply() {
        Task {
            await model.applySettings(
                agentId: agentId,
                gatewayUrl: gatewayUrl,
                dashboardUrl: dashboardUrl.isEmpty ? nil : dashboardUrl,
                tlsSkipVerify: tlsSkipVerify
            )
        }
    }
}

// ── General ─────────────────────────────────────────────────────────────

private struct GeneralPane: View {
    @Environment(AgentModel.self) private var model

    @State private var startAtLogin = LoginItem.isEnabled
    @State private var loginItemError: String?

    var body: some View {
        Form {
            Section("Startup") {
                Toggle("Start at login", isOn: $startAtLogin)
                    .onChange(of: startAtLogin) { _, enabled in
                        do {
                            try LoginItem.setEnabled(enabled)
                            loginItemError = nil
                        } catch {
                            // Put the switch back where it actually is.
                            startAtLogin = LoginItem.isEnabled
                            loginItemError = error.localizedDescription
                        }
                    }
                if LoginItem.isBlockedByUser {
                    HStack(spacing: 8) {
                        Text("Turned off in System Settings.")
                            .font(.system(size: Typo.caption))
                            .foregroundStyle(Palette.warn)
                        Button("Open Login Items") { LoginItem.openSystemSettings() }
                            .controlSize(.small)
                    }
                }
                if let loginItemError {
                    Text(loginItemError)
                        .font(.system(size: Typo.caption))
                        .foregroundStyle(Palette.deny)
                }
                Text(
                    "The app reconnects this Mac's MCP servers after a restart. Closing the "
                        + "window keeps it running in the menu bar; quitting stops the servers."
                )
                .font(.system(size: Typo.caption))
                .foregroundStyle(Palette.text3)
            }

            Section("Files") {
                LabeledContent("Configuration") {
                    HStack(spacing: 8) {
                        Text(model.config?.configPath ?? "—")
                            .font(.system(size: Typo.caption, design: .monospaced))
                            .lineLimit(1)
                            .truncationMode(.head)
                            .textSelection(.enabled)
                        Button("Reveal") { reveal() }
                            .controlSize(.small)
                    }
                }
            }
        }
        .formStyle(.grouped)
    }

    private func reveal() {
        guard let path = model.config?.configPath else { return }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
    }
}

// ── Updates ─────────────────────────────────────────────────────────────

private struct UpdatesPane: View {
    @Environment(AgentModel.self) private var model

    var body: some View {
        Form {
            Section("Version") {
                LabeledContent("Installed", value: model.updater.currentVersion)
                LabeledContent("Agent core", value: model.coreVersion)
                if let checked = model.updater.lastChecked {
                    LabeledContent("Last checked", value: Format.dateTime(checked))
                }
            }

            Section("Updates") {
                switch model.updater.state {
                case .idle, .upToDate:
                    Text("You are up to date.")
                        .foregroundStyle(Palette.text3)
                case .checking:
                    HStack(spacing: 8) {
                        ProgressView().controlSize(.small)
                        Text("Checking…")
                    }
                case let .available(release):
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Version \(release.version) is available")
                            .font(.system(size: Typo.small, weight: .medium))
                        if let notes = release.notes, !notes.isEmpty {
                            ScrollView {
                                Text(notes)
                                    .font(.system(size: Typo.caption))
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            .frame(maxHeight: 120)
                        }
                        if model.updater.canInstall {
                            Button("Download and install") {
                                Task { await model.updater.requestUpdate(release) }
                            }
                            .buttonStyle(.glassProminent)
                        } else {
                            Text(
                                "This build has no update signing key, so it will not install "
                                    + "updates automatically. Download the new version from GitHub."
                            )
                            .font(.system(size: Typo.caption))
                            .foregroundStyle(Palette.warn)
                        }
                    }
                case let .downloading(progress):
                    ProgressView(value: progress) { Text("Downloading…") }
                case let .readyToInstall(release):
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Version \(release.version) is downloaded and ready")
                            .font(.system(size: Typo.small, weight: .medium))
                        Button("Restart and update") {
                            Task { await model.updater.requestUpdate(release) }
                        }
                        .buttonStyle(.glassProminent)
                    }
                case .readyToRelaunch:
                    // Installing quits and reopens the app on its own, so this
                    // is a state you see in passing rather than one to act on.
                    HStack(spacing: 8) {
                        ProgressView().controlSize(.small)
                        Text("Relaunching…")
                    }
                case let .failed(message):
                    Text(message)
                        .font(.system(size: Typo.caption))
                        .foregroundStyle(Palette.deny)
                }

                Button("Check now") { Task { await model.updater.check() } }
            }
        }
        .formStyle(.grouped)
    }
}

// ── About ───────────────────────────────────────────────────────────────

private struct AboutPane: View {
    @Environment(AgentModel.self) private var model

    var body: some View {
        VStack(spacing: 12) {
            BrandTile(size: 76, cornerRadius: 21)

            Text("MCP Gateway Agent")
                .font(.system(size: Typo.medium, weight: .semibold))
            Text("Version \(model.updater.currentVersion) · core \(model.coreVersion)")
                .font(.system(size: Typo.caption))
                .foregroundStyle(Palette.text3)

            Text(
                "Keeps this Mac's MCP servers connected to your gateway, so tools running here "
                    + "are available to every client the gateway serves."
            )
            .font(.system(size: Typo.small))
            .foregroundStyle(Palette.text3)
            .multilineTextAlignment(.center)
            .frame(maxWidth: 340)

            HStack(spacing: 10) {
                Link(
                    "Documentation",
                    destination: URL(
                        string: "https://github.com/SidPad03/unified-mcp-gateway/tree/main/docs")!)
                Link(
                    "Report an issue",
                    destination: URL(
                        string: "https://github.com/SidPad03/unified-mcp-gateway/issues")!)
            }
            .font(.system(size: Typo.caption))

            Spacer()
            Text("Apache-2.0")
                .font(.system(size: Typo.micro))
                .foregroundStyle(Palette.text3)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
