// speakly-syscap: ScreenCaptureKit audio helper.
//
// Streams per-app or whole-system audio to stdout as framed messages:
//   [u32 le type][u32 le len][payload]
//   type 1 AUDIO  = u64 le pts_ns + f32le mono samples at --rate
//   type 2 STATUS = utf8 JSON, e.g. {"event":"started"}
//   type 3 ERROR  = utf8 JSON, e.g. {"error":"...","code":"permission-denied"}
// stdin accepts one JSON command per line: {"cmd":"stop"}.
// `--list-apps` prints a plain JSON array (no framing) and exits.

import AVFoundation
import CoreGraphics
import CoreMedia
import Foundation
import ScreenCaptureKit

// MARK: - Frame writing

let frameQueue = DispatchQueue(label: "speakly.frames")

func writeFrame(type: UInt32, payload: Data) {
    var data = Data(capacity: 8 + payload.count)
    withUnsafeBytes(of: type.littleEndian) { data.append(contentsOf: $0) }
    withUnsafeBytes(of: UInt32(payload.count).littleEndian) { data.append(contentsOf: $0) }
    data.append(payload)
    frameQueue.sync {
        FileHandle.standardOutput.write(data)
    }
}

func writeStatus(_ object: [String: Any]) {
    if let json = try? JSONSerialization.data(withJSONObject: object) {
        writeFrame(type: 2, payload: json)
    }
}

func writeError(_ message: String, code: String) {
    if let json = try? JSONSerialization.data(withJSONObject: ["error": message, "code": code]) {
        writeFrame(type: 3, payload: json)
    }
}

func writeAudio(ptsNs: UInt64, samples: [Float]) {
    guard !samples.isEmpty else { return }
    var payload = Data(capacity: 8 + samples.count * 4)
    withUnsafeBytes(of: ptsNs.littleEndian) { payload.append(contentsOf: $0) }
    samples.withUnsafeBufferPointer { buf in
        payload.append(Data(buffer: buf))  // Float32 little-endian on arm64
    }
    writeFrame(type: 1, payload: payload)
}

// MARK: - Argument parsing

struct Args {
    var listApps = false
    var system = false
    var bundleIds: [String] = []
    var rate: Double = 16_000
}

func parseArgs() -> Args {
    var args = Args()
    var it = CommandLine.arguments.dropFirst().makeIterator()
    while let a = it.next() {
        switch a {
        case "--list-apps": args.listApps = true
        case "--system": args.system = true
        case "--bundle-id":
            if let v = it.next() { args.bundleIds.append(v) }
        case "--rate":
            if let v = it.next(), let r = Double(v) { args.rate = r }
        default:
            FileHandle.standardError.write("unknown arg: \(a)\n".data(using: .utf8)!)
        }
    }
    return args
}

// MARK: - Audio conversion

/// Extract mono Float32 samples and the source sample rate from an SCK buffer.
/// Handles interleaved and non-interleaved float32 layouts.
func monoSamples(from sb: CMSampleBuffer) -> ([Float], Double)? {
    guard let fmt = sb.formatDescription,
        let asbd = fmt.audioStreamBasicDescription
    else { return nil }
    let rate = asbd.mSampleRate
    let interleavedChannels = Int(asbd.mChannelsPerFrame)
    var result: [Float] = []
    do {
        try sb.withAudioBufferList { abl, _ in
            let buffers = Array(abl)
            guard !buffers.isEmpty else { return }
            if buffers.count == 1 && interleavedChannels > 1 {
                // Interleaved stereo in one buffer.
                let b = buffers[0]
                guard let base = b.mData?.assumingMemoryBound(to: Float.self) else { return }
                let total = Int(b.mDataByteSize) / MemoryLayout<Float>.size
                let frames = total / interleavedChannels
                result.reserveCapacity(frames)
                for f in 0..<frames {
                    var s: Float = 0
                    for c in 0..<interleavedChannels { s += base[f * interleavedChannels + c] }
                    result.append(s / Float(interleavedChannels))
                }
            } else {
                // One buffer per channel.
                let chans: [UnsafeBufferPointer<Float>] = buffers.compactMap { b in
                    guard let base = b.mData?.assumingMemoryBound(to: Float.self) else { return nil }
                    return UnsafeBufferPointer(
                        start: base, count: Int(b.mDataByteSize) / MemoryLayout<Float>.size)
                }
                guard let first = chans.first else { return }
                if chans.count == 1 {
                    result = Array(first)
                } else {
                    let n = first.count
                    result.reserveCapacity(n)
                    for i in 0..<n {
                        var s: Float = 0
                        var used = 0
                        for c in chans where i < c.count {
                            s += c[i]
                            used += 1
                        }
                        result.append(used > 0 ? s / Float(used) : 0)
                    }
                }
            }
        }
    } catch {
        return nil
    }
    return (result, rate)
}

/// Streaming downsampler to the target rate; keeps converter state across calls.
final class Downsampler {
    private let outFormat: AVAudioFormat
    private var inFormat: AVAudioFormat?
    private var converter: AVAudioConverter?

    init(targetRate: Double) {
        outFormat = AVAudioFormat(
            commonFormat: .pcmFormatFloat32, sampleRate: targetRate, channels: 1,
            interleaved: false)!
    }

    func convert(_ samples: [Float], rate: Double) -> [Float] {
        if samples.isEmpty { return [] }
        if rate == outFormat.sampleRate { return samples }
        if inFormat?.sampleRate != rate {
            inFormat = AVAudioFormat(
                commonFormat: .pcmFormatFloat32, sampleRate: rate, channels: 1, interleaved: false)
            converter = inFormat.flatMap { AVAudioConverter(from: $0, to: outFormat) }
        }
        guard let conv = converter, let inF = inFormat,
            let inBuf = AVAudioPCMBuffer(
                pcmFormat: inF, frameCapacity: AVAudioFrameCount(samples.count))
        else { return [] }
        inBuf.frameLength = AVAudioFrameCount(samples.count)
        samples.withUnsafeBufferPointer { src in
            inBuf.floatChannelData![0].update(from: src.baseAddress!, count: samples.count)
        }
        let capacity =
            AVAudioFrameCount(Double(samples.count) * outFormat.sampleRate / rate) + 64
        guard let outBuf = AVAudioPCMBuffer(pcmFormat: outFormat, frameCapacity: capacity) else {
            return []
        }
        var fed = false
        var convError: NSError?
        let status = conv.convert(to: outBuf, error: &convError) { _, outStatus in
            if fed {
                outStatus.pointee = .noDataNow
                return nil
            }
            fed = true
            outStatus.pointee = .haveData
            return inBuf
        }
        if status == .error { return [] }
        return Array(
            UnsafeBufferPointer(start: outBuf.floatChannelData![0], count: Int(outBuf.frameLength)))
    }
}

// MARK: - Stream plumbing

final class StreamDelegate: NSObject, SCStreamDelegate {
    func stream(_ stream: SCStream, didStopWithError error: Error) {
        writeError(error.localizedDescription, code: "stream-failed")
        exit(1)
    }
}

final class AudioOutput: NSObject, SCStreamOutput {
    let downsampler: Downsampler
    init(targetRate: Double) {
        downsampler = Downsampler(targetRate: targetRate)
    }

    func stream(
        _ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
        of type: SCStreamOutputType
    ) {
        guard type == .audio, sampleBuffer.isValid else { return }
        guard let (mono, rate) = monoSamples(from: sampleBuffer) else { return }
        let out = downsampler.convert(mono, rate: rate)
        let pts = CMSampleBufferGetPresentationTimeStamp(sampleBuffer)
        let ptsNs = pts.isNumeric ? UInt64(max(0, pts.seconds) * 1_000_000_000) : 0
        writeAudio(ptsNs: ptsNs, samples: out)
    }
}

// MARK: - Modes

func runListApps() async {
    do {
        let content = try await SCShareableContent.excludingDesktopWindows(
            false, onScreenWindowsOnly: false)
        let apps = content.applications.map { app in
            [
                "bundleId": app.bundleIdentifier,
                "name": app.applicationName,
                "pid": Int(app.processID),
            ] as [String: Any]
        }
        let json = try JSONSerialization.data(withJSONObject: apps)
        FileHandle.standardOutput.write(json)
        FileHandle.standardOutput.write("\n".data(using: .utf8)!)
        exit(0)
    } catch {
        writeError(error.localizedDescription, code: "shareable-content")
        exit(2)
    }
}

// Retained globally so the stream lives for the whole process.
var activeStream: SCStream?
let streamDelegate = StreamDelegate()
var audioOutput: AudioOutput?
let audioQueue = DispatchQueue(label: "speakly.audio")

func runCapture(_ args: Args) async {
    do {
        let content = try await SCShareableContent.excludingDesktopWindows(
            false, onScreenWindowsOnly: false)
        guard let display = content.displays.first else {
            writeError("no display available", code: "no-display")
            exit(1)
        }
        let filter: SCContentFilter
        if args.system {
            filter = SCContentFilter(
                display: display, excludingApplications: [], exceptingWindows: [])
        } else {
            let wanted = Set(args.bundleIds)
            let apps = content.applications.filter { wanted.contains($0.bundleIdentifier) }
            if apps.isEmpty {
                writeError(
                    "no running app matches: \(args.bundleIds.joined(separator: ","))",
                    code: "app-not-found")
                exit(1)
            }
            filter = SCContentFilter(display: display, including: apps, exceptingWindows: [])
        }

        let cfg = SCStreamConfiguration()
        cfg.capturesAudio = true
        cfg.excludesCurrentProcessAudio = true
        cfg.sampleRate = 48_000
        cfg.channelCount = 2
        // SCK requires a video config even for audio-only use; keep it minimal.
        cfg.width = 2
        cfg.height = 2
        cfg.minimumFrameInterval = CMTime(value: 1, timescale: 1)
        cfg.showsCursor = false

        let output = AudioOutput(targetRate: args.rate)
        audioOutput = output
        let stream = SCStream(filter: filter, configuration: cfg, delegate: streamDelegate)
        try stream.addStreamOutput(output, type: .audio, sampleHandlerQueue: audioQueue)
        activeStream = stream
        try await stream.startCapture()
        writeStatus(["event": "started", "rate": args.rate])
    } catch {
        writeError(error.localizedDescription, code: "stream-failed")
        exit(1)
    }
}

func watchStdin() {
    DispatchQueue.global().async {
        while let line = readLine(strippingNewline: true) {
            if line.contains("\"stop\"") {
                Task {
                    try? await activeStream?.stopCapture()
                    writeStatus(["event": "stopped"])
                    exit(0)
                }
            }
        }
        // stdin closed: the parent is gone — shut down.
        Task {
            try? await activeStream?.stopCapture()
            exit(0)
        }
    }
}

// MARK: - Entry

let args = parseArgs()

if args.listApps {
    Task { await runListApps() }
    dispatchMain()
}

guard CGPreflightScreenCaptureAccess() else {
    if ProcessInfo.processInfo.environment["SPEAKLY_PROMPT"] == "1" {
        CGRequestScreenCaptureAccess()
    }
    writeError("screen recording permission not granted", code: "permission-denied")
    exit(2)
}

guard args.system || !args.bundleIds.isEmpty else {
    writeError("pass --system or at least one --bundle-id", code: "bad-args")
    exit(2)
}

watchStdin()
Task { await runCapture(args) }
dispatchMain()
