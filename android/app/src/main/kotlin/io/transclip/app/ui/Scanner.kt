package io.transclip.app.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.zxing.BarcodeFormat
import com.journeyapps.barcodescanner.BarcodeCallback
import com.journeyapps.barcodescanner.DecoratedBarcodeView
import com.journeyapps.barcodescanner.DefaultDecoderFactory

/**
 * Camera preview that reads QR codes, embedded in the layout (not a full-screen activity), so it
 * follows the rotation of the screen like any other view. Give it a square size for a 1:1 scanner.
 * The camera runs only while the screen is resumed.
 */
@Composable
fun ScannerBox(modifier: Modifier, onResult: (String) -> Unit) {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val view = remember {
        DecoratedBarcodeView(context).apply {
            barcodeView.decoderFactory = DefaultDecoderFactory(listOf(BarcodeFormat.QR_CODE))
            setStatusText("")
        }
    }
    DisposableEffect(view, lifecycle) {
        var delivered = false
        view.decodeContinuous(
            BarcodeCallback { result ->
                val text = result.text
                if (!delivered && !text.isNullOrEmpty()) {
                    delivered = true
                    view.pause()
                    onResult(text)
                }
            },
        )
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> if (!delivered) view.resume()
                Lifecycle.Event.ON_PAUSE -> view.pause()
                else -> {}
            }
        }
        lifecycle.addObserver(observer)
        if (lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) view.resume()
        onDispose {
            lifecycle.removeObserver(observer)
            view.pause()
        }
    }
    AndroidView({ view }, modifier)
}
