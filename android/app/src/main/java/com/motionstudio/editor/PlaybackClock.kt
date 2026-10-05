package com.motionstudio.editor

/** Choreographer timestamps can precede a play action processed in the same
 * display cycle. Such a callback keeps the starting frame instead of rewinding. */
internal fun playbackFrame(startFrame:Double,startNanos:Long,frameTimeNanos:Long,fps:Int,frames:Int):Double {
    require(fps>0&&frames>0)
    val seed=if(startFrame.isFinite())((startFrame%frames)+frames)%frames else 0.0
    val elapsed=(frameTimeNanos-startNanos).coerceAtLeast(0L)/1e9
    return (seed+elapsed*fps)%frames
}
