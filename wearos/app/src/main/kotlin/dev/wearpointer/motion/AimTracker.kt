package dev.wearpointer.motion

import kotlin.math.PI
import kotlin.math.asin
import kotlin.math.sqrt

/**
 * Gravity elevation plus yaw about the vertical axis.
 *
 * Adapted from WatchPointer, Copyright (c) 2026 Umar Afzal, MIT.
 * https://github.com/umarz317/WatchPointer
 *
 * Android's gravity sensor points the opposite way from Apple's
 * CMDeviceMotion.gravity. When an axis points at the sky it reads about
 * +9.81, so up = normalize(gravity). Device +Y is the 12 o'clock edge.
 * Elevation is asin(up.y) and does not drift. Heading integrates
 * dot(gyro, up) * dt. Positive rotation about the upward axis is a turn
 * to the left.
 *
 * [deadZoneRadPerSec] defaults to 0. WatchPointer removed a dead zone
 * because it walked the cursor out of line. The same math is unit-tested
 * in the Rust core.
 */
class AimTracker(private val deadZoneRadPerSec: Double = 0.0) {
    private var heading = 0.0
    private var elevation = 0.0
    private var elevationZero = 0.0
    private var lastT: Double? = null

    fun center() {
        heading = 0.0
        elevationZero = elevation
    }

    /**
     * [timestampNs] uses the sensor clock (elapsed realtime nanos).
     * [gravity] is m/s². [gyro] is rad/s.
     */
    fun push(timestampNs: Long, gravity: FloatArray, gyro: FloatArray) {
        val t = timestampNs / 1_000_000_000.0
        val dt = lastT?.let { prev -> (t - prev).coerceIn(0.0, 0.05) } ?: 0.0
        lastT = t

        val gx = gravity[0].toDouble()
        val gy = gravity[1].toDouble()
        val gz = gravity[2].toDouble()
        val norm = sqrt(gx * gx + gy * gy + gz * gz).coerceAtLeast(1e-6)
        val upX = gx / norm
        val upY = gy / norm
        val upZ = gz / norm
        elevation = asin(upY.coerceIn(-1.0, 1.0))

        var yaw = gyro[0] * upX + gyro[1] * upY + gyro[2] * upZ
        if (kotlin.math.abs(yaw) < deadZoneRadPerSec) {
            yaw = 0.0
        }
        heading += yaw * dt
    }

    fun leftCdeg(): Int = cdeg(signedRemainder(heading, PI * 2.0))

    fun upCdeg(): Int = cdeg(elevation - elevationZero)

    private fun cdeg(radians: Double): Int {
        val hundredths = (radians * 180.0 / PI * 100.0).let { kotlin.math.round(it) }
        return hundredths.toInt().coerceIn(Short.MIN_VALUE.toInt(), Short.MAX_VALUE.toInt())
    }

    private fun signedRemainder(value: Double, modulus: Double): Double {
        var r = value % modulus
        if (r > modulus / 2.0) r -= modulus
        if (r < -modulus / 2.0) r += modulus
        return r
    }
}
