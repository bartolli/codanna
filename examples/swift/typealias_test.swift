// Test case for typealias vs struct with same name
// This mirrors the Kingfisher ImageResource pattern

@available(*, deprecated)
public typealias ImageResource = KF.ImageResource

enum KF {
    public struct ImageResource: Resource {
        public var cacheKey: String
    }
}

protocol Resource {
    var cacheKey: String { get }
}
