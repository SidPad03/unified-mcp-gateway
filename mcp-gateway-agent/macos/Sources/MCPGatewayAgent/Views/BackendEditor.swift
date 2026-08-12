import SwiftUI

/// Add or edit one local MCP server.
///
/// The Test button is the point of this sheet. A backend whose command is
/// misspelled used to be discovered when the agent next started and then only as
/// a line in a log; here it is caught before the configuration is written, and
/// the result names the tools it found so you know you pointed at the right
/// thing.
struct BackendEditor: View {
    enum Mode {
        case create
        case edit(BackendView)
    }

    @Environment(AgentModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    let mode: Mode

    @State private var draft = BackendConfig()
    @State private var argsText = ""
    @State private var envPairs: [Pair] = []
    @State private var headerPairs: [Pair] = []
    @State private var testState = TestState.idle
    @State private var saveError: String?
    @State private var saving = false

    private enum TestState: Equatable {
        case idle
        case running
        case passed(BackendTestResult)
        case failed(String)
    }

    private struct Pair: Identifiable, Equatable {
        let id = UUID()
        var key = ""
        var value = ""
        /// Hidden everywhere — this sheet, the backend list, the dashboard —
        /// until it is unmasked here and saved. Off by default: most of what
        /// goes in an environment is a path or a flag.
        var masked = false

        /// True while `value` is the placeholder standing in for a value the
        /// agent would not hand over. The field shows empty, and leaving it that
        /// way keeps what is on disk.
        var isHidden: Bool { value == maskedPlaceholder }
    }

    private var isEditing: Bool {
        if case .edit = mode { return true }
        return false
    }

    private var originalName: String? {
        if case let .edit(backend) = mode { return backend.name }
        return nil
    }

    var body: some View {
        VStack(spacing: 0) {
            title
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    identity
                    transportFields
                    pairEditor(
                        title: "Environment",
                        note: "Passed to the process. Secrets belong here rather than in the "
                            + "command — mask one with the lock and it is hidden everywhere, "
                            + "here and on the dashboard, until you unmask it.",
                        pairs: $envPairs
                    )
                    if !draft.isStdio {
                        pairEditor(
                            title: "Headers",
                            note: "Sent with every request to this backend.",
                            pairs: $headerPairs
                        )
                    }
                    testResult
                }
                .padding(20)
            }
            Divider()
            footer
        }
        .frame(width: 560, height: 620)
        .background(Palette.canvas)
        .onAppear(perform: load)
    }

    // ── Sections ────────────────────────────────────────────────────────

    private var title: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(isEditing ? "Edit backend" : "Add backend")
                    .font(.system(size: 15, weight: .semibold))
                Text("An MCP server running on this Mac")
                    .font(.system(size: Typo.caption))
                    .foregroundStyle(Palette.text3)
            }
            Spacer()
        }
        .padding(18)
    }

    private var identity: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHeader("Identity")
            LabeledField(label: "Name", hint: "Namespaces this backend's tools as name__tool") {
                TextField("blender", text: $draft.name)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(size: Typo.body, design: .monospaced))
            }
            Picker("Transport", selection: $draft.transport) {
                Text("stdio (a local process)").tag("stdio")
                Text("HTTP (a local server)").tag("http")
            }
            .pickerStyle(.segmented)
            .labelsHidden()
        }
    }

    @ViewBuilder
    private var transportFields: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHeader(draft.isStdio ? "Command" : "Endpoint")
            if draft.isStdio {
                LabeledField(label: "Command", hint: "Found on your login shell's PATH") {
                    TextField(
                        "uvx",
                        text: .init(
                            get: { draft.command ?? "" },
                            set: { draft.command = $0.isEmpty ? nil : $0 }
                        )
                    )
                    .textFieldStyle(.roundedBorder)
                    .font(.system(size: Typo.body, design: .monospaced))
                }
                LabeledField(label: "Arguments", hint: "One per line") {
                    TextEditor(text: $argsText)
                        .font(.system(size: Typo.small, design: .monospaced))
                        .frame(height: 66)
                        .scrollContentBackground(.hidden)
                        .padding(4)
                        .background(.quaternary.opacity(0.4), in: .rect(cornerRadius: Radius.control))
                }
            } else {
                LabeledField(label: "URL", hint: "The server's MCP endpoint") {
                    TextField(
                        "http://127.0.0.1:3010/mcp",
                        text: .init(
                            get: { draft.url ?? "" },
                            set: { draft.url = $0.isEmpty ? nil : $0 }
                        )
                    )
                    .textFieldStyle(.roundedBorder)
                    .font(.system(size: Typo.body, design: .monospaced))
                }
            }
        }
    }

    private func pairEditor(title: String, note: String, pairs: Binding<[Pair]>) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeader(title) {
                Button {
                    pairs.wrappedValue.append(Pair())
                } label: {
                    Image(systemName: "plus").font(.system(size: Typo.micro, weight: .bold))
                }
                .buttonStyle(.plain)
                .foregroundStyle(Palette.beam)
            }
            Text(note)
                .font(.system(size: Typo.caption))
                .foregroundStyle(Palette.text3)

            ForEach(pairs) { $pair in
                HStack(spacing: 7) {
                    TextField("KEY", text: $pair.key)
                        .textFieldStyle(.roundedBorder)
                        .font(.system(size: Typo.small, design: .monospaced))
                        .frame(width: 165)
                    valueField(for: $pair)
                    Button {
                        pair.masked.toggle()
                    } label: {
                        Image(systemName: pair.masked ? "lock.fill" : "lock.open")
                            .font(.system(size: Typo.small))
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(pair.masked ? Palette.warn : Palette.text3)
                    .help(
                        pair.masked
                            ? "Masked — hidden here and on the dashboard until you unmask it"
                            : "Plain text — shown wherever this backend is displayed"
                    )
                    Button {
                        pairs.wrappedValue.removeAll { $0.id == pair.id }
                    } label: {
                        Image(systemName: "minus.circle")
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(Palette.text3)
                }
            }
        }
    }

    /// A masked value is typed blind; a plain one is ordinary text.
    ///
    /// A masked variable that already exists arrives as the placeholder, so the
    /// field shows empty and says why. Typing replaces it; leaving it alone
    /// keeps what is on disk.
    private func valueField(for pair: Binding<Pair>) -> some View {
        let hidden = pair.wrappedValue.isHidden
        let prompt =
            switch (hidden, pair.wrappedValue.masked) {
            case (false, _): "value"
            case (true, true): "hidden — type to replace"
            // Unmasked but not yet saved: the value is still on disk, and the
            // next time this sheet opens it will be in the field.
            case (true, false): "shown here once you save"
            }
        let text = Binding(
            get: { hidden ? "" : pair.wrappedValue.value },
            set: { pair.wrappedValue.value = $0 }
        )
        return Group {
            if pair.wrappedValue.masked {
                SecureField(prompt, text: text)
            } else {
                TextField(prompt, text: text)
            }
        }
        .textFieldStyle(.roundedBorder)
        .font(.system(size: Typo.small, design: .monospaced))
    }

    @ViewBuilder
    private var testResult: some View {
        switch testState {
        case .idle:
            EmptyView()
        case .running:
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Starting the backend and listing its tools…")
                    .font(.system(size: Typo.small))
                    .foregroundStyle(Palette.text3)
            }
        case let .passed(result):
            VStack(alignment: .leading, spacing: 6) {
                Label(
                    "\(result.toolCount) tools in \(Format.duration(result.tookMs))",
                    systemImage: "checkmark.circle.fill"
                )
                .font(.system(size: Typo.small, weight: .medium))
                .foregroundStyle(Palette.beam)
                if !result.tools.isEmpty {
                    Text(result.tools.prefix(12).joined(separator: ", ")
                        + (result.tools.count > 12 ? ", …" : ""))
                        .font(.system(size: Typo.caption, design: .monospaced))
                        .foregroundStyle(Palette.text3)
                        .lineLimit(3)
                }
            }
            .padding(11)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Palette.beam.opacity(0.1), in: .rect(cornerRadius: Radius.row))
        case let .failed(message):
            InlineBanner(text: message)
        }
    }

    private var footer: some View {
        HStack(spacing: 9) {
            Button("Test connection") { test() }
                .buttonStyle(.glass)
                .disabled(testState == .running || saving)
            if let saveError {
                Text(saveError)
                    .font(.system(size: Typo.caption))
                    .foregroundStyle(Palette.deny)
                    .lineLimit(2)
            }
            Spacer()
            Button("Cancel") { dismiss() }
                .buttonStyle(.glass)
                .keyboardShortcut(.cancelAction)
            Button(isEditing ? "Save" : "Add") { save() }
                .buttonStyle(.glassProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(saving)
        }
        .padding(16)
    }

    // ── Data ────────────────────────────────────────────────────────────

    private func load() {
        if case let .edit(backend) = mode {
            draft = BackendConfig(existing: backend)
            argsText = backend.args.joined(separator: "\n")
            envPairs = backend.env.map(pair(from:))
            headerPairs = backend.headers.map(pair(from:))
        }
    }

    private func pair(from setting: BackendSetting) -> Pair {
        Pair(key: setting.key, value: setting.value ?? maskedPlaceholder, masked: setting.masked)
    }

    /// Fold the editors back into the config.
    ///
    /// A plain variable is sent as typed — what the field shows is what is
    /// stored. A masked one whose field was left empty is sent as the
    /// placeholder, which the core turns back into the value already on disk;
    /// that is what lets an edit to the arguments leave a secret alone, and what
    /// brings the value back when the mask is cleared.
    private func assemble() -> BackendConfig {
        var config = draft
        config.name = draft.name.trimmingCharacters(in: .whitespaces)
        config.args =
            argsText
            .split(separator: "\n")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }

        func dictionary(from pairs: [Pair]) -> [String: String] {
            var result: [String: String] = [:]
            for pair in pairs {
                let key = pair.key.trimmingCharacters(in: .whitespaces)
                guard !key.isEmpty else { continue }
                result[key] =
                    pair.masked && pair.value.isEmpty ? maskedPlaceholder : pair.value
            }
            return result
        }

        func maskedKeys(from pairs: [Pair]) -> [String] {
            pairs
                .filter(\.masked)
                .map { $0.key.trimmingCharacters(in: .whitespaces) }
                .filter { !$0.isEmpty }
        }

        config.env = dictionary(from: envPairs)
        config.maskedEnv = maskedKeys(from: envPairs)
        config.headers = draft.isStdio ? [:] : dictionary(from: headerPairs)
        config.maskedHeaders = draft.isStdio ? [] : maskedKeys(from: headerPairs)
        if draft.isStdio { config.url = nil } else { config.command = nil }
        return config
    }

    private func test() {
        testState = .running
        Task {
            do {
                testState = .passed(try await model.testBackend(assemble()))
            } catch {
                testState = .failed(error.localizedDescription)
            }
        }
    }

    private func save() {
        saving = true
        saveError = nil
        Task {
            defer { saving = false }
            do {
                let config = assemble()
                if let originalName {
                    try await model.updateBackend(name: originalName, to: config)
                } else {
                    try await model.addBackend(config)
                }
                dismiss()
            } catch {
                saveError = error.localizedDescription
            }
        }
    }
}

private struct LabeledField<Content: View>: View {
    let label: String
    var hint: String?
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(.system(size: Typo.caption, weight: .medium))
                .foregroundStyle(Palette.text3)
            content
            if let hint {
                Text(hint)
                    .font(.system(size: Typo.micro))
                    .foregroundStyle(Palette.text3)
            }
        }
    }
}
