package com.webbook

import android.app.Application

class WebBookApp : Application() {

    override fun onCreate() {
        super.onCreate()
        instance = this
    }

    companion object {
        lateinit var instance: WebBookApp
            private set
    }
}
