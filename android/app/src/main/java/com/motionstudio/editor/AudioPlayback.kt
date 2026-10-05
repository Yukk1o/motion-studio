package com.motionstudio.editor

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.os.Handler
import android.os.HandlerThread
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.atomic.AtomicLong

/** Each play owns a frozen mix. Pause invalidates queued audio before seeking. */
internal class AudioPlayback(private val onError:(String)->Unit) {
    private val thread=HandlerThread("studio-audio").apply{start()}
    private val worker=Handler(thread.looper)
    private val generation=AtomicLong()
    @Volatile private var track:AudioTrack?=null
    @Volatile private var startSample=0L
    @Volatile private var totalSamples=0L
    fun frame(fps:Int):Double?=track?.let{t->
        val played=t.playbackHeadPosition.toLong() and 0xffffffffL
        ((startSample+played)%totalSamples.coerceAtLeast(1)).toDouble()*fps/48000.0
    }
    fun start(handle:Long,start:Long,total:Long,onReady:()->Unit) {
        val ticket=generation.incrementAndGet()
        worker.post {
            var output:AudioTrack?=null
            try {
                if(ticket!=generation.get())return@post
                val minimum=AudioTrack.getMinBufferSize(48000,AudioFormat.CHANNEL_OUT_STEREO,AudioFormat.ENCODING_PCM_FLOAT)
                check(minimum>0){"设备不支持声音播放"}
                output=AudioTrack.Builder().setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MOVIE).build())
                    .setAudioFormat(AudioFormat.Builder().setSampleRate(48000).setChannelMask(AudioFormat.CHANNEL_OUT_STEREO).setEncoding(AudioFormat.ENCODING_PCM_FLOAT).build())
                    .setBufferSizeInBytes(maxOf(minimum,2048*8)).setTransferMode(AudioTrack.MODE_STREAM).build()
                check(output.state==AudioTrack.STATE_INITIALIZED){"声音设备初始化失败"}
                val bytes=ByteBuffer.allocateDirect(1024*8).order(ByteOrder.LITTLE_ENDIAN)
                val samples=FloatArray(2048)
                var at=start.coerceIn(0,total-1)
                startSample=at;totalSamples=total;track=output
                var begun=false
                while(ticket==generation.get()) {
                    val data=nativeData(MediaBridge.readFrozenPcmInto(handle,at,minOf(1024L,total-at).toInt(),bytes))
                    val count=data.getInt("frames")
                    check(count>0){"声音缓存为空"}
                    bytes.position(0);bytes.asFloatBuffer().get(samples,0,count*2)
                    if(!begun){output.play();begun=true;onReady()}
                    var offset=0
                    while(offset<count*2&&ticket==generation.get()) {
                        val written=output.write(samples,offset,count*2-offset,AudioTrack.WRITE_BLOCKING)
                        check(written>0){"声音输出失败：$written"};offset+=written
                    }
                    at=(at+count)%total
                }
            }catch(e:Throwable){if(ticket==generation.get())onError(e.message?:"声音播放失败")}
            finally {
                if(track===output)track=null
                runCatching{output?.pause()};runCatching{output?.flush()};runCatching{output?.release()}
                runCatching{MediaBridge.releaseFrozenAudio(handle)}
            }
        }
    }
    fun stop(){generation.incrementAndGet();runCatching{track?.pause()};runCatching{track?.flush()}}
    fun close(){stop();worker.post{thread.quitSafely()}}
}
