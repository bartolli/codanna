public class DataStreamRequest {
    public struct Stream: Sendable {
        public let value: Int
    }

    public enum Event: Sendable {
        case stream
        case complete
    }
}
