import SwiftUI

/// First run: one field and one button.
///
/// This replaces the old four-step wizard, and the step it removes is the one
/// that used to cause the trouble — "create an API key in the dashboard, copy
/// it, paste it here". Sign-in happens in a real browser against the gateway,
/// so an existing dashboard session is reused and a password manager works.
/// Backends are added afterwards, from a running app, rather than being a wall
/// standing between the user and a working agent.
struct WelcomeView: View {
    @Environment(AgentModel.self) private var model

    @State private var gateway = ""
    @State private var agentId = AgentModel.defaultAgentID()
    @State private var allowInsecureTLS = false
    @State private var showingAdvanced = false

    private var canSignIn: Bool {
        !gateway.trimmingCharacters(in: .whitespaces).isEmpty && !model.signingIn
    }

    var body: some View {
        ZStack {
            Palette.canvas.ignoresSafeArea()

            VStack(spacing: 0) {
                Spacer(minLength: 0)

                GlassEffectContainer(spacing: 18) {
                    VStack(spacing: 18) {
                        header
                        Card {
                            VStack(alignment: .leading, spacing: 14) {
                                field
                                if showingAdvanced { advanced }
                                actions
                            }
                        }
                        .frame(width: 420)
                    }
                }

                if let error = model.lastError {
                    InlineBanner(text: error) { model.lastError = nil }
                        .frame(width: 420)
                        .padding(.top, 14)
                }

                Spacer(minLength: 0)
                footer
            }
            .padding(40)
        }
    }

    private var header: some View {
        VStack(spacing: 10) {
            BrandTile(size: 64, cornerRadius: 18)
            Text("Connect this Mac to your gateway")
                .font(.system(size: Typo.large, weight: .semibold))
                .tracking(-0.2)
            Text("The app keeps this machine's MCP servers available to the gateway.")
                .font(.system(size: Typo.small))
                .foregroundStyle(Palette.text3)
        }
        .padding(.bottom, 4)
    }

    private var field: some View {
        VStack(alignment: .leading, spacing: 6) {
            SectionHeader("Gateway address")
            TextField("gateway.example.com", text: $gateway)
                .textFieldStyle(.plain)
                .font(.system(size: Typo.body))
                .padding(.horizontal, 11)
                .padding(.vertical, 9)
                .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: Radius.row))
                .onSubmit { if canSignIn { signIn() } }
            Text("The address of your dashboard. The scheme and path are worked out for you.")
                .font(.system(size: Typo.caption))
                .foregroundStyle(Palette.text3)
        }
    }

    private var advanced: some View {
        VStack(alignment: .leading, spacing: 10) {
            Divider()
            VStack(alignment: .leading, spacing: 6) {
                SectionHeader("Machine name")
                TextField("this-mac", text: $agentId)
                    .textFieldStyle(.plain)
                    .font(.system(size: Typo.body, design: .monospaced))
                    .padding(.horizontal, 11)
                    .padding(.vertical, 8)
                    .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: Radius.row))
                Text("How this Mac appears in the dashboard.")
                    .font(.system(size: Typo.caption))
                    .foregroundStyle(Palette.text3)
            }
            Toggle("Skip TLS certificate verification", isOn: $allowInsecureTLS)
                .font(.system(size: Typo.small))
            if allowInsecureTLS {
                Text(
                    "Only for a gateway with a self-signed certificate on a network you trust. "
                        + "It disables the check that proves you are talking to the right server."
                )
                .font(.system(size: Typo.caption))
                .foregroundStyle(Palette.warn)
            }
        }
    }

    private var actions: some View {
        VStack(spacing: 10) {
            Button {
                signIn()
            } label: {
                HStack(spacing: 7) {
                    if model.signingIn {
                        ProgressView().controlSize(.small)
                    }
                    Text(model.signingIn ? "Waiting for the browser…" : "Sign in with your gateway")
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 3)
            }
            .buttonStyle(.glassProminent)
            .disabled(!canSignIn)

            Button(showingAdvanced ? "Hide options" : "More options") {
                withAnimation(.snappy(duration: 0.2)) { showingAdvanced.toggle() }
            }
            .buttonStyle(.plain)
            .font(.system(size: Typo.caption))
            .foregroundStyle(Palette.text3)
        }
    }

    private var footer: some View {
        Text("A browser window opens so you can sign in to the gateway.")
            .font(.system(size: Typo.caption))
            .foregroundStyle(Palette.text3)
    }

    private func signIn() {
        Task {
            await model.signIn(
                gateway: gateway,
                agentId: agentId,
                allowInsecureTLS: allowInsecureTLS
            )
        }
    }
}
