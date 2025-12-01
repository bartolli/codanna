<?php
// PHP readonly class - exact pattern from research report
readonly class ImmutableUser {
    public function __construct(
        public string $name
    ) {}
}
