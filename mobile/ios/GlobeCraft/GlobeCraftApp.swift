import SwiftUI

@main
struct GlobeCraftApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}

/// Phase 0: the globe full screen, a statistics line (renderer, frame size and time,
/// downloads) and a Relief switch, to measure the renderer on the iPad. The app's real
/// screens (search, layers, details in a sheet) come in Phase 2.
struct ContentView: View {
    @StateObject private var model = GlobeModel()

    var body: some View {
        ZStack {
            if let globe = model.globe {
                GlobeRepresentable(globe: globe, stats: $model.stats)
                    .ignoresSafeArea()
                VStack(alignment: .leading) {
                    Text(model.stats)
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.white)
                        .padding(6)
                        .background(.black.opacity(0.55), in: RoundedRectangle(cornerRadius: 6))
                    Spacer()
                    HStack {
                        Spacer()
                        Toggle("Relief", isOn: $model.relief)
                            .fixedSize()
                            .padding(.horizontal, 12)
                            .padding(.vertical, 8)
                            .background(.regularMaterial, in: Capsule())
                    }
                }
                .padding()
            } else {
                Text("The globe could not start.")
                    .foregroundStyle(.white)
            }
        }
        .background(.black)
    }
}

final class GlobeModel: ObservableObject {
    let globe: OpaquePointer?
    @Published var stats = ""
    @Published var relief = true {
        didSet { gc_set_relief(globe, relief) }
    }

    init() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let dir = support.appendingPathComponent("GlobeCraft", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        globe = gc_globe_new(dir.path)
    }

    deinit {
        gc_globe_free(globe)
    }
}
