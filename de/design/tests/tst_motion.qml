import QtQuick
import QtTest
import Fern.Design

TestCase {
    name: "Motion"

    function test_spring_curve() {
        compare(Motion.sample(0), 0)
        fuzzyCompare(Motion.sample(1), 1, 1e-9)
        let last = 0
        for (let i = 1; i <= 100; i++) {
            const v = Motion.sample(i / 100)
            verify(v >= last, "monotonic at " + i)
            verify(v <= 1 + 1e-9, "no overshoot at " + i)
            last = v
        }
        // Critically damped from rest: a gentle start, most of the way by
        // half time.
        verify(Motion.sample(0.05) < 0.1)
        verify(Motion.sample(0.5) > 0.9)
    }

    function test_presets_follow_the_compositor() {
        // 1.47 response times: 0.25 s, 0.42 s, 0.6 s (de/comp/src/spring.rs).
        compare(Motion.snappy, 368)
        compare(Motion.smooth, 617)
        compare(Motion.gentle, 882)
    }
}
