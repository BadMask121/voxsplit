import Foundation
import os

/// What the engine printed: a protocol event (stdout) or free-form text (stdout or stderr).
public enum EngineOutput: Sendable, Equatable {
    case event(EngineEvent)
    case log(String)
}

/// Runs `voxsplit-engine` as a child process.
public struct EngineRunner: Sendable {
    public let executable: URL

    public init(executable: URL) {
        self.executable = executable
    }

    /// Spawns the engine and streams its output to `onOutput` line by line, in order per
    /// stream. Returns the exit status.
    ///
    /// `arguments` is passed as an argv array (no shell), so paths with spaces, quotes or
    /// unicode need no escaping. Cancelling the calling task sends SIGTERM; the engine then
    /// finishes its current chunk, deletes its partial files and exits with 130.
    @concurrent
    public func run(
        arguments: [String],
        onOutput: @escaping @Sendable (EngineOutput) async -> Void
    ) async throws -> Int32 {
        let stdout = Pipe()
        let stderr = Pipe()
        let process = Process()
        process.executableURL = executable
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = stdout
        process.standardError = stderr

        let handle = ProcessHandle(process)
        try handle.launch()

        // Our copies of the write ends must be closed, or the reads below never see EOF.
        try? stdout.fileHandleForWriting.close()
        try? stderr.fileHandleForWriting.close()
        let outLines = Self.lines(from: stdout.fileHandleForReading)
        let errLines = Self.lines(from: stderr.fileHandleForReading)

        // The readers deliberately aren't children of this task: after a cancel they keep
        // draining until EOF, so the engine's final `cancelled` event still arrives.
        let readers = Task {
            await withDiscardingTaskGroup { group in
                group.addTask {
                    for await line in outLines {
                        await onOutput(EngineEvent(line: line).map(EngineOutput.event) ?? .log(line))
                    }
                }
                group.addTask {
                    for await line in errLines {
                        await onOutput(.log(line))
                    }
                }
            }
        }

        return await withTaskCancellationHandler {
            await readers.value
            return await handle.exitStatus()
        } onCancel: {
            handle.terminate()
        }
    }

    /// Lines from a pipe as soon as they're written. (`FileHandle.bytes.lines` buffers pipe
    /// reads, so a progress stream smaller than its buffer only arrives at EOF.)
    private static func lines(from file: FileHandle) -> AsyncStream<String> {
        let (stream, continuation) = AsyncStream.makeStream(of: String.self, bufferingPolicy: .unbounded)
        let pending = OSAllocatedUnfairLock(initialState: Data())

        file.readabilityHandler = { file in
            let chunk = file.availableData
            if chunk.isEmpty { // EOF
                file.readabilityHandler = nil
                let rest = pending.withLock { data in defer { data.removeAll() }; return data }
                if !rest.isEmpty {
                    continuation.yield(Self.decode(rest))
                }
                continuation.finish()
                return
            }
            let complete: [Data] = pending.withLock { data in
                data.append(chunk)
                var lines: [Data] = []
                while let newline = data.firstIndex(of: UInt8(ascii: "\n")) {
                    lines.append(data[data.startIndex..<newline])
                    data.removeSubrange(data.startIndex...newline)
                }
                return lines
            }
            for line in complete {
                continuation.yield(Self.decode(line))
            }
        }
        return stream
    }

    private static func decode(_ line: Data) -> String {
        var text = String(decoding: line, as: UTF8.self)
        if text.hasSuffix("\r") { text.removeLast() }
        return text
    }
}

/// Owns the `Process` behind a lock so cancellation (which runs concurrently with the
/// body) can terminate it safely, and turns its termination callback into an `await`.
/// `Process` isn't `Sendable`; it is only ever touched while holding the lock, which is
/// what makes the unchecked lock state sound. (`Mutex` would say this directly but needs macOS 15.)
private final class ProcessHandle: Sendable {
    private struct State {
        var process: Process
        var status: Int32?
        var waiter: CheckedContinuation<Int32, Never>?
    }

    private let state: OSAllocatedUnfairLock<State>

    init(_ process: sending Process) {
        state = OSAllocatedUnfairLock(uncheckedState: State(process: process))
    }

    func launch() throws {
        try state.withLockUnchecked { s in
            s.process.terminationHandler = { [weak self] finished in
                self?.finished(with: finished.terminationStatus)
            }
            try s.process.run()
        }
    }

    /// SIGTERM, so the engine can clean up; SIGKILL if it hasn't exited 5 s later.
    func terminate() {
        let pid: pid_t? = state.withLockUnchecked { s in
            guard s.process.isRunning else { return nil }
            s.process.terminate()
            return s.process.processIdentifier
        }
        guard let pid else { return }
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(5))
            guard let self, self.state.withLockUnchecked({ $0.status == nil }) else { return }
            kill(pid, SIGKILL)
        }
    }

    func exitStatus() async -> Int32 {
        await withCheckedContinuation { continuation in
            let ready: Int32? = state.withLockUnchecked { s in
                if let status = s.status { return status }
                s.waiter = continuation
                return nil
            }
            if let ready {
                continuation.resume(returning: ready)
            }
        }
    }

    private func finished(with status: Int32) {
        let waiter: CheckedContinuation<Int32, Never>? = state.withLockUnchecked { s in
            s.status = status
            defer { s.waiter = nil }
            return s.waiter
        }
        waiter?.resume(returning: status)
    }
}
