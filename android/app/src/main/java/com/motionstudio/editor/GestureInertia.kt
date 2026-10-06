package com.motionstudio.editor

import android.view.ViewConfiguration
import androidx.compose.animation.core.AnimationState
import androidx.compose.animation.core.animateDecay
import androidx.compose.animation.core.exponentialDecay
import androidx.compose.runtime.*
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlin.math.abs

/** Stop every editor fling before a new touch can start another command. */
internal class GestureInertiaGroup {
    private val motions=mutableSetOf<GestureInertia>()
    fun register(motion:GestureInertia){motions.add(motion)}
    fun unregister(motion:GestureInertia){motions.remove(motion)}
    fun stop(){motions.toList().forEach{it.stop()}}
}

internal class GestureInertia(private val scope:CoroutineScope) {
    private var job:Job?=null
    private var generation=0
    private var finished:(()->Unit)?=null

    // Complete synchronously so a new gesture cannot open before the old one closes.
    fun stop() {
        generation++
        job?.cancel();job=null
        val completion=finished;finished=null
        completion?.invoke()
    }

    fun start(velocity:Float,minimumVelocity:Float,friction:Float,onDelta:(Float)->Boolean,onFinished:()->Unit={}) {
        stop()
        if(!velocity.isFinite()||abs(velocity)<minimumVelocity){onFinished();return}
        val run=++generation
        finished=onFinished
        job=scope.launch {
            try {
                var previous=0f
                AnimationState(initialValue=0f,initialVelocity=velocity).animateDecay(exponentialDecay(frictionMultiplier=friction)) {
                    val delta=value-previous;previous=value
                    if(delta!=0f&&!onDelta(delta))cancelAnimation()
                }
            }finally {
                if(generation==run) {
                    job=null
                    val completion=finished;finished=null
                    completion?.invoke()
                }
            }
        }
    }
}

@Composable internal fun rememberGestureInertia(group:GestureInertiaGroup?=null):GestureInertia {
    val scope=rememberCoroutineScope()
    val motion=remember(scope){GestureInertia(scope)}
    DisposableEffect(motion,group) {
        group?.register(motion)
        onDispose{motion.stop();group?.unregister(motion)}
    }
    return motion
}

internal data class FlingLimits(val minimum:Float,val maximum:Float)
@Composable internal fun rememberFlingLimits():FlingLimits {
    val context=LocalContext.current
    return remember(context){ViewConfiguration.get(context).let{FlingLimits(it.scaledMinimumFlingVelocity.toFloat(),it.scaledMaximumFlingVelocity.toFloat())}}
}

/** Holding the finger still before release is a deliberate stop. */
internal fun releaseVelocity(velocity:Float,releasedAt:Long,lastMovedAt:Long,limits:FlingLimits):Float =
    if(releasedAt-lastMovedAt>100||!velocity.isFinite())0f else velocity.coerceIn(-limits.maximum,limits.maximum)
