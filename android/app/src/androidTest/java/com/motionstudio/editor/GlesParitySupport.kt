package com.motionstudio.editor

import android.graphics.Bitmap
import android.graphics.Color
import org.json.JSONObject
import org.junit.Assert.assertTrue
import java.nio.ByteBuffer
import kotlin.math.abs
import kotlin.math.pow

/** Compare the exporter's actual unencoded FBO to a straight-alpha native PNG. */
internal fun glesParity(reference: Bitmap, pixels: ByteBuffer): JSONObject {
    fun linear(v: Double) = if (v <= .04045) v / 12.92 else ((v + .055) / 1.055).pow(2.4)
    fun encode(v: Double) = if (v <= .0031308) v * 12.92 else 1.055 * v.pow(1 / 2.4) - .055
    var rgb = 0.0; var alpha = 0.0; var foreground = 0.0; var active = 0; var visible = 0
    for (y in 0 until reference.height) for (x in 0 until reference.width) {
        val p = ((reference.height - 1 - y) * reference.width + x) * 4
        val a = (pixels.get(p + 3).toInt() and 255) / 255.0
        val expected = reference.getPixel(x, y)
        val channels = intArrayOf(Color.red(expected), Color.green(expected), Color.blue(expected))
        if (Color.alpha(expected) > 0) visible++
        for (c in 0..2) {
            val v = (pixels.get(p + c).toInt() and 255) / 255.0
            val straight = if (a == 0.0) 0.0 else encode((linear(v) / a).coerceIn(0.0, 1.0)) * 255
            val error = abs(straight - channels[c]); rgb += error
            if (Color.alpha(expected) > 32) { foreground += error; active++ }
        }
        alpha += abs(a * 255 - Color.alpha(expected))
    }
    val count = reference.width * reference.height
    val report = JSONObject().put("rgbMae", rgb / (count * 3)).put("alphaMae", alpha / count)
        .put("foregroundRgbMae", foreground / active.coerceAtLeast(1)).put("visiblePixels", visible)
    assertTrue("Empty reference: $report", visible > 50)
    assertTrue(report.toString(), report.getDouble("rgbMae") <= 3 && report.getDouble("alphaMae") <= 3 && report.getDouble("foregroundRgbMae") <= 3)
    return report
}
