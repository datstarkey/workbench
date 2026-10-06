package com.workbench.speechinput

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.speech.RecognizerIntent
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/** Delegate speech capture to the device's recognizer; no audio crosses the chat API. */
@TauriPlugin
class SpeechInputPlugin(private val activity: Activity) : Plugin(activity) {
  private var recognizing = false

  @Command
  fun recognize(invoke: Invoke) {
    activity.runOnUiThread {
      if (recognizing) {
        invoke.reject("Voice input is already open")
        return@runOnUiThread
      }
      val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
        .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
        .putExtra(RecognizerIntent.EXTRA_PROMPT, "Dictate a message")
        .putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 1)
      recognizing = true
      try {
        // Starting directly avoids Android 11 package-visibility restrictions on resolveActivity.
        startActivityForResult(invoke, intent, "recognitionResult")
      } catch (e: ActivityNotFoundException) {
        recognizing = false
        invoke.reject("No speech input app is installed. Enable or install a speech recognition service in Android settings.")
      } catch (e: Exception) {
        recognizing = false
        invoke.reject(e.message ?: "Couldn't open voice input")
      }
    }
  }

  @ActivityCallback
  fun recognitionResult(invoke: Invoke, result: ActivityResult) {
    recognizing = false
    when (result.resultCode) {
      Activity.RESULT_OK -> {
        val text = result.data?.getStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS)
          ?.firstOrNull()?.trim()?.takeIf { it.isNotEmpty() }
        if (text == null) invoke.reject("No speech was recognized. Try again.")
        else invoke.resolve(JSObject().put("text", text))
      }
      Activity.RESULT_CANCELED -> invoke.resolve(JSObject().put("text", org.json.JSONObject.NULL))
      else -> invoke.reject("Speech recognition failed. Try again.")
    }
  }
}
