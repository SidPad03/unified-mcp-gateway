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
                        note: isEditing
                            ? "Values are not shown: the agent never hands them back out. "
                                + "Leave one blank to keep it as it is."
                            : "Passed to the process. Secrets belong here rather than in the command.",
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
                        .frame(width: 180)
                    SecureField("value", text: $pair.value)
                        .textFieldStyle(.roundedBorder)
                        .font(.system(size: Typo.small, design: .monospaced))
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
            envPairs = backend.envKeys.map { Pair(key: $0, value: "") }
            headerPairs = backend.headerKeys.map { Pair(key: $0, value: "") }
        }
    }

    /// Fold the editors back into the config.
    ///
    /// A pair with an empty value on an *existing* backend means "leave this
    /// one alone", so it is dropped here and the value already on disk survives
    /// the round trip. On a new backend an empty value is just an empty value.
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
                if pair.value.isEmpty && isEditing { continue }
                result[key] = pair.value
            }
            return result
        }

        config.env = dictionary(from: envPairs)
        config.headers = draft.isStdio ? [:] : dictionary(from: headerPairs)
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
