package com.motionstudio.editor

import java.nio.ByteBuffer

/** Ordered split triangles and point-specific picking from the same native scene. */
object GeometryBridge {
    init { System.loadLibrary("motion_engine") }
    @JvmStatic external fun sampleGeometryInto(id:Long,frame:Double,parameters:ByteBuffer,vertices:ByteBuffer):String
    @JvmStatic external fun hitCandidates(id:Long,x:Double,y:Double):String
}
