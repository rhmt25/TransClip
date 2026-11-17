# Keep serializable message classes and the BouncyCastle classes used by :proto.
-keep,includedescriptorclasses class io.transclip.proto.** { *; }
-keepclassmembers class io.transclip.proto.** { *** Companion; }
-keep class org.bouncycastle.crypto.** { *; }
-keep class org.bouncycastle.math.ec.rfc7748.** { *; }
-dontwarn org.bouncycastle.**
