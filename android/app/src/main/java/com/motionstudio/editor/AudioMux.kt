package com.motionstudio.editor

import android.media.*
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.roundToInt

/** Disk-backed AAC pass keeps memory bounded; both tracks retain one sample clock. */
internal object AudioMux {
    fun addAudio(video:File,handle:Long,samples:Long,cancelled:AtomicBoolean) {
        val audio=File(video.parentFile,video.nameWithoutExtension+"-audio.tmp.m4a")
        val combined=File(video.parentFile,video.nameWithoutExtension+"-mux.tmp.mp4")
        try {
            val delay=encode(audio,handle,samples,cancelled)
            val extracts=listOf(MediaExtractor(),MediaExtractor())
            val mux=MediaMuxer(combined.absolutePath,MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
            var running=false
            try {
                extracts[0].setDataSource(video.absolutePath);extracts[1].setDataSource(audio.absolutePath)
                val indexes=extracts.mapIndexed{i,e->
                    val input=(0 until e.trackCount).first{e.getTrackFormat(it).getString(MediaFormat.KEY_MIME)!!.startsWith(if(i==0)"video/"else"audio/")}
                    e.selectTrack(input);mux.addTrack(e.getTrackFormat(input))
                }
                mux.start();running=true
                var bytes=ByteBuffer.allocateDirect(1024*1024)
                val info=MediaCodec.BufferInfo();val ended=BooleanArray(2)
                while(ended.any{!it}) {
                    check(!cancelled.get()){"导出已取消"}
                    val i=extracts.indices.filter{!ended[it]}.minByOrNull{extracts[it].sampleTime}!!
                    val e=extracts[i]
                    if(e.sampleTime<0){ended[i]=true;continue}
                    val size=e.sampleSize
                    check(size in 0..16L*1024*1024){"编码样本超过预算"}
                    if(bytes.capacity()<size)bytes=ByteBuffer.allocateDirect(size.toInt())
                    bytes.clear();val actual=e.readSampleData(bytes,0)
                    check(actual>=0){"编码样本读取失败"}
                    info.set(0,actual,e.sampleTime,if(e.sampleFlags and MediaExtractor.SAMPLE_FLAG_SYNC!=0)MediaCodec.BUFFER_FLAG_KEY_FRAME else 0)
                    mux.writeSampleData(indexes[i],bytes,info);e.advance()
                }
                info.set(0,0,samples*1_000_000/48000,MediaCodec.BUFFER_FLAG_END_OF_STREAM)
                mux.writeSampleData(indexes[0],ByteBuffer.allocate(0),info)
                info.presentationTimeUs=(samples+delay)*1_000_000/48000
                mux.writeSampleData(indexes[1],ByteBuffer.allocate(0),info)
                mux.stop();running=false
            }finally{if(running)runCatching{mux.stop()};mux.release();extracts.forEach{it.release()}}
            Mp4AudioTiming.trim(combined,delay,samples)
            check(!cancelled.get()){"导出已取消"}
            Files.move(combined.toPath(),video.toPath(),StandardCopyOption.REPLACE_EXISTING)
        }finally{audio.delete();combined.delete()}
    }
    private fun encode(file:File,handle:Long,total:Long,cancelled:AtomicBoolean):Int {
        // Pin the platform software AAC-LC implementation when available. Its
        // 48 kHz stereo priming is verified against decoded exported samples;
        // vendor codecs must report their own delay rather than silently using 0.
        val names=MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.filter{it.isEncoder&&it.supportedTypes.any{type->type=="audio/mp4a-latm"}}.map{it.name}
        val platformName=listOf("c2.android.aac.encoder","OMX.google.aac.encoder").firstOrNull{it in names}
        val codec=if(platformName!=null)MediaCodec.createByCodecName(platformName)else MediaCodec.createEncoderByType("audio/mp4a-latm")
        val mux=MediaMuxer(file.absolutePath,MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
        var started=false;var codecStarted=false;var track=-1;var sample=0L;var inputEnded=false;var outputEnded=false
        var firstPts:Long?=null;var delay=if(platformName!=null)2048 else -1
        val pcm=ByteBuffer.allocateDirect(1024*8).order(ByteOrder.LITTLE_ENDIAN)
        val info=MediaCodec.BufferInfo();var lastOutput=System.nanoTime()
        try {
            val format=MediaFormat.createAudioFormat("audio/mp4a-latm",48000,2).apply {
                setInteger(MediaFormat.KEY_AAC_PROFILE,MediaCodecInfo.CodecProfileLevel.AACObjectLC)
                setInteger(MediaFormat.KEY_BIT_RATE,192000);setInteger(MediaFormat.KEY_PCM_ENCODING,AudioFormat.ENCODING_PCM_16BIT)
            }
            codec.configure(format,null,null,MediaCodec.CONFIGURE_FLAG_ENCODE);codec.start();codecStarted=true
            while(!outputEnded) {
                check(!cancelled.get()){"导出已取消"}
                check(System.nanoTime()-lastOutput<15_000_000_000L){"声音编码器未及时输出"}
                if(!inputEnded) {
                    val index=codec.dequeueInputBuffer(1000)
                    if(index>=0) {
                        val input=codec.getInputBuffer(index)!!.apply{clear();order(ByteOrder.LITTLE_ENDIAN)}
                        val inputTotal=total+delay.coerceAtLeast(0)
                        if(sample==inputTotal){codec.queueInputBuffer(index,0,0,sample*1_000_000/48000,MediaCodec.BUFFER_FLAG_END_OF_STREAM);inputEnded=true}
                        else {
                            val count=minOf(1024L,inputTotal-sample,(input.remaining()/4).toLong()).toInt()
                            val sourceCount=minOf(count.toLong(),(total-sample).coerceAtLeast(0)).toInt()
                            if(sourceCount>0)check(nativeData(MediaBridge.readFrozenPcmInto(handle,sample,sourceCount,pcm)).getInt("frames")==sourceCount){"声音缓存读取失败"}
                            // Flush the encoder's delayed tail with silence; the edit list
                            // retains exactly the composition's original samples.
                            for(i in 0 until count*2)input.putShort(if(i<sourceCount*2)(pcm.getFloat(i*4).coerceIn(-1f,1f)*32767f).roundToInt().toShort()else 0)
                            codec.queueInputBuffer(index,0,count*4,sample*1_000_000/48000,0);sample+=count
                        }
                    }
                }
                val output=codec.dequeueOutputBuffer(info,1000)
                if(output==MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                    check(!started){"声音编码格式重复变更"}
                    val out=codec.outputFormat
                    if(out.containsKey("encoder-delay"))delay=out.getInteger("encoder-delay")
                    track=mux.addTrack(out);mux.start();started=true
                }else if(output>=0) {
                    try {
                        if(info.size>0&&info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG==0) {
                            check(started){"声音编码格式未就绪"}
                            if(firstPts==null){firstPts=info.presentationTimeUs;if(delay<0&&firstPts!!<0)delay=(-firstPts!!*48000/1_000_000.0).roundToInt()}
                            val buffer=codec.getOutputBuffer(output)!!
                            val shifted=info.presentationTimeUs-firstPts!!
                            check(shifted>=0){"声音编码时间戳错误"}
                            val sampleInfo=MediaCodec.BufferInfo().apply{set(info.offset,info.size,shifted,info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM.inv())}
                            mux.writeSampleData(track,buffer,sampleInfo)
                        }
                        outputEnded=info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM!=0
                    }finally{codec.releaseOutputBuffer(output,false)}
                    lastOutput=System.nanoTime()
                }
            }
            check(started&&firstPts!=null&&delay>=0){"声音编码未完成"}
            info.set(0,0,(total+delay)*1_000_000/48000,MediaCodec.BUFFER_FLAG_END_OF_STREAM)
            mux.writeSampleData(track,ByteBuffer.allocate(0),info);mux.stop();started=false
            return delay
        }finally{if(started)runCatching{mux.stop()};mux.release();if(codecStarted)runCatching{codec.stop()};codec.release()}
    }
}
