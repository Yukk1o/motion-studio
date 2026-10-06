package com.motionstudio.editor;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.res.AssetFileDescriptor;
import android.database.Cursor;
import android.net.Uri;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.util.Arrays;

/** Test APK can run in its own process without the target APK's Kotlin runtime. */
public final class AudioFixtureProvider extends ContentProvider {
    @Override public boolean onCreate() { return true; }
    @Override public AssetFileDescriptor openAssetFile(Uri uri, String mode) throws FileNotFoundException {
        if (!"r".equals(mode)) throw new FileNotFoundException("Read only");
        String name = uri.getLastPathSegment();
        if (Arrays.asList("silent-24fps.mp4", "sound-24fps.mp4", "audio-delayed.mp4", "rotated-90.mp4", "variable.mp4", "preview-1080p.mp4").contains(name)) {
            try { return getContext().getAssets().openFd("video/" + name); }
            catch (IOException e) { throw new FileNotFoundException(e.toString()); }
        }
        if ("slow.wav".equals(name)) {
            try { Thread.sleep(1500); } catch (InterruptedException e) {
                Thread.currentThread().interrupt(); throw new FileNotFoundException("Interrupted");
            }
            name = "tone-stereo-48000.wav";
        }
        if (!Arrays.asList("tone-stereo-48000.wav", "tone-stereo-48000.m4a", "tone-mono-44100.mp3", "invalid.bin").contains(name))
            throw new FileNotFoundException("Unknown audio fixture");
        try { return getContext().getAssets().openFd(name); }
        catch (IOException e) { throw new FileNotFoundException(e.toString()); }
    }
    @Override public String getType(Uri uri) { return "application/octet-stream"; }
    @Override public Cursor query(Uri uri, String[] projection, String selection, String[] selectionArgs, String sortOrder) { return null; }
    @Override public Uri insert(Uri uri, ContentValues values) { throw new UnsupportedOperationException("Read only"); }
    @Override public int delete(Uri uri, String selection, String[] selectionArgs) { throw new UnsupportedOperationException("Read only"); }
    @Override public int update(Uri uri, ContentValues values, String selection, String[] selectionArgs) { throw new UnsupportedOperationException("Read only"); }
}
