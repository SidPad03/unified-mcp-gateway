import SwiftUI

/// The local MCP servers this Mac puts behind the gateway.
///
/// Everything here works while connected. Adding, removing, enabling or
/// restarting a backend takes effect immediately and triggers a debounced
/// re-registration with the gateway — the old agent needed a full restart for
/// any of it.
///
/// Laid out to match the dashboard's Backends page: one figure leads, then a
/// railed list where each row's left edge carries its health. The two are the
/// same screen in two runtimes.
struct BackendsView: View {
    @Environment(AgentModel.self) private var model

    @State private var editing: EditorTarget?
    @State private var pendingRemoval: BackendView?
    @State private var expanded: String?

    private enum EditorTarget: Identifiable {
        case new
        case existing(BackendView)

        var id: String {
            switch self {
            case .new: "new"
            case let .existing(backend): backend.name
            }
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Metrics.gutter) {
                header

                if let error = model.lastError {
                    InlineBanner(text: error) { model.lastError = nil }
                }

                if model.backends.isEmpty {
                    Card {
                        EmptyState(
                            icon: "server.rack",
                            title: "No local MCP servers",
                            message:
                                "Add the MCP servers running on this Mac and the gateway will be "
                                + "able to route tool calls to them.",
                            action: ("Add backend", { editing = .new })
                        )
                    }
                } else {
                    summary
                    RailList {
                        ForEach(Array(model.backends.enumerated()), id: \.element.id) {
                            index, backend in
                            BackendRow(
                                backend: backend,
                                isLast: index == model.backends.count - 1
                                    && expanded != backend.name,
                                isExpanded: expanded == backend.name,
                                onToggleExpand: {
                                    expanded = expanded == backend.name ? nil : backend.name
                                },
                                onEdit: { editing = .existing(backend) },
                                onRemove: { pendingRemoval = backend }
                            )
                        }
                    }
                }
            }
            .padding(Metrics.pagePadding)
            .padding(.top, 16)  // clears the traffic lights and sidebar toggle when the sidebar is hidden
        }
        .sheet(item: $editing) { target in
            switch target {
            case .new:
                BackendEditor(mode: .create)
            case let .existing(backend):
                BackendEditor(mode: .edit(backend))
            }
        }
        .alert(
            "Delete \(pendingRemoval?.name ?? "")?",
            isPresented: .init(
                get: { pendingRemoval != nil },
                set: { if !$0 { pendingRemoval = nil } }
            )
        ) {
            Button("Delete", role: .destructive) {
                if let name = pendingRemoval?.name {
                    Task { await model.removeBackend(name) }
                }
                pendingRemoval = nil
            }
            Button("Cancel", role: .cancel) { pendingRemoval = nil }
        } message: {
            Text(
                "Its process stops and its tools are withdrawn from the gateway. "
                    + "The configuration is deleted."
            )
        }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            PageTitle(
                title: "Backends",
                subtitle: "The MCP servers running on this Mac, and what the gateway can see of them."
            )
            Spacer(minLength: 16)
            Button {
                editing = .new
            } label: {
                Label("Add backend", systemImage: "plus")
            }
            .buttonStyle(.glassProminent)
            .controlSize(.small)
        }
    }

    /// Equal columns at one size. Which figure matters most is a judgement that
    /// changes with what you came to the page for, so it is carried by order
    /// rather than by making one of them larger.
    private var summary: some View {
        Card {
            HStack(alignment: .top, spacing: 24) {
                Stat(
                    value: Format.count(model.backends.reduce(0) { $0 + $1.toolCount }),
                    label: "Tools exposed"
                )

                Stat(value: "\(model.backends.count)", label: "Backends")
                Stat(
                    value: "\(model.backends.filter { $0.status == .ready }.count)",
                    label: "Running",
                    tint: Palette.beam
                )
                let trouble = model.backends.filter {
                    $0.status == .failed || $0.status == .crashed
                }
                if !trouble.isEmpty {
                    Stat(value: "\(trouble.count)", label: "Failed", tint: Palette.deny)
                }
                Stat(
                    value: "\(model.backends.filter { !$0.enabled }.count)",
                    label: "Disabled"
                )
            }
        }
    }
}

// ── One backend ─────────────────────────────────────────────────────────

private struct BackendRow: View {
    @Environment(AgentModel.self) private var model
    let backend: BackendView
    let isLast: Bool
    let isExpanded: Bool
    let onToggleExpand: () -> Void
    let onEdit: () -> Void
    let onRemove: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            RailRow(tone: backend.status.tone, isLast: isLast && !isExpanded) {
                HStack(spacing: 9) {
                    Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
                        .font(.system(size: 8, weight: .semibold))
                        .foregroundStyle(Palette.text4)
                        .frame(width: 9)
                    Mono(backend.name, size: Typo.small, weight: .medium)
                    StatusLabel(
                        tone: backend.status.tone,
                        text: backend.status.label,
                        pulsing: backend.status == .starting
                    )
                    Badge(backend.transport)
                    Text("\(backend.toolCount) tools")
                        .font(.system(size: Typo.micro))
                        .monospacedDigit()
                        .foregroundStyle(Palette.text4)
                    if !backend.enabled { Badge("disabled") }
                }
                .contentShape(.rect)
                .onTapGesture(perform: onToggleExpand)
            } trailing: {
                controls
            }
            .opacity(backend.enabled ? 1 : 0.6)

            if isExpanded {
                detail
            }
        }
    }

    /// Quiet icon buttons, not `.glass`. Glass is chrome — see the rule on
    /// `Card` — and three live materials per row meant thirty re-compositing
    /// surfaces on a machine with ten servers, inside a scrolling list. A
    /// hover fill is the same affordance the sidebar's rows use, at the cost
    /// of a rectangle.
    private var controls: some View {
        HStack(spacing: 4) {
            Toggle(
                "Enabled",
                isOn: .init(
                    get: { backend.enabled },
                    set: { enabled in
                        Task { await model.setBackendEnabled(backend.name, enabled: enabled) }
                    }
                )
            )
            .toggleStyle(.switch)
            .labelsHidden()
            .controlSize(.mini)
            .help(backend.enabled ? "Disable this backend" : "Enable this backend")
            .padding(.trailing, 4)

            RowIconButton(icon: "arrow.clockwise", help: "Restart") {
                Task { await model.restartBackend(backend.name) }
            }
            .disabled(!backend.enabled)

            RowIconButton(icon: "slider.horizontal.3", help: "Edit", action: onEdit)

            RowIconButton(icon: "trash", role: .destructive, help: "Delete", action: onRemove)
        }
    }

    private var detail: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let error = backend.error {
                InlineBanner(text: error)
            }

            HStack(alignment: .top, spacing: 10) {
                FieldLabel("Command")
                Mono(backend.subtitle, size: Typo.caption, color: Palette.text2)
                    .textSelection(.enabled)
                    .lineLimit(2)
                Spacer(minLength: 0)
            }

            facts

            if !backend.tools.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    FieldLabel("Tools")
                    VStack(alignment: .leading, spacing: 3) {
                        ForEach(backend.tools) { tool in
                            HStack(alignment: .top, spacing: 10) {
                                Mono(tool.bareName, size: Typo.caption, color: Palette.text)
                                    .frame(width: 190, alignment: .leading)
                                Text(tool.description)
                                    .font(.system(size: Typo.caption))
                                    .foregroundStyle(Palette.text3)
                                    .lineLimit(1)
                                Spacer(minLength: 0)
                            }
                        }
                    }
                }
            }
        }
        .padding(.horizontal, 15)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Palette.inset)
        .overlay(alignment: .bottom) {
            if !isLast {
                Rectangle().fill(Palette.lineSoft).frame(height: 1)
            }
        }
    }

    private var facts: some View {
        HStack(alignment: .top, spacing: 22) {
            if let pid = backend.pid {
                Fact(label: "PID", value: "\(pid)")
            }
            if backend.status == .ready {
                Fact(label: "Uptime", value: Format.uptime(backend.uptimeSecs))
            }
            if backend.restarts > 0 {
                Fact(
                    label: "Restarts",
                    value: "\(backend.restarts)",
                    tint: backend.restarts > 3 ? Palette.warn : Palette.text
                )
            }
            if !backend.envKeys.isEmpty {
                Fact(label: "Env", value: backend.envKeys.joined(separator: ", "))
            }
            if !backend.headerKeys.isEmpty {
                Fact(label: "Headers", value: backend.headerKeys.joined(separator: ", "))
            }
            Spacer(minLength: 0)
        }
    }
}

private struct Fact: View {
    let label: String
    let value: String
    var tint: Color = Palette.text

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            FieldLabel(label)
            Mono(value, size: Typo.caption, color: tint)
                .lineLimit(1)
        }
    }
}

private struct RowIconButton: View {
    let icon: String
    var role: ButtonRole?
    let help: String
    let action: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(role: role, action: action) {
            Image(systemName: icon)
                .font(.system(size: Typo.small, weight: .medium))
                .foregroundStyle(
                    hovering ? (role == .destructive ? Palette.deny : Palette.text) : Palette.text3
                )
                .frame(width: 24, height: 22)
                .background(
                    RoundedRectangle(cornerRadius: Radius.control)
                        .fill(Color.primary.opacity(hovering ? 0.07 : 0))
                )
                .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .help(help)
        .accessibilityLabel(help)
    }
}
