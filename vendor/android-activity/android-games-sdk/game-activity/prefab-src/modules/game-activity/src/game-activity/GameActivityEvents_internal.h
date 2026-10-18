/*
 * Copyright (C) 2022 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

/**
 * @addtogroup GameActivity Game Activity Events Internal
 * These functions are internal details of Game Activity Events.
 * Please do not rely on anything in this file as this can be changed
 * without notice.
 * @{
 */

/**
 * @file GameActivityEvents_internal.h
 */
#ifndef ANDROID_GAME_SDK_GAME_ACTIVITY_EVENTS_INTERNAL_H
#define ANDROID_GAME_SDK_GAME_ACTIVITY_EVENTS_INTERNAL_H

#include <jni.h>

#ifdef __cplusplus
extern "C" {
#endif

/** \brief Performs necessary initialization steps for GameActivityEvents.
 *
 * User must call this function before calling any other functions of this unit.
 * If you use GameActivity it will call this function for you.
 */
void GameActivityEventsInit(JNIEnv* env);

/**
 * \brief Convert a Java `MotionEvent` to a `GameActivityMotionEvent`.
 *
 * This is done automatically by the GameActivity: see `onTouchEvent` to set
 * a callback to consume the received events.
 * This function can be used if you re-implement events handling in your own
 * activity.
 * Ownership of out_event is maintained by the caller.
 * Note that we pass as much information from Java Activity as possible
 * to avoid extra JNI calls.
 */
void GameActivityMotionEvent_fromJava(JNIEnv* env, jobject motionEvent,
                                      GameActivityMotionEvent* out_event, int pointerCount,
                                      int historySize);

/**
 * \brief Convert a Java `KeyEvent` to a `GameActivityKeyEvent`.
 *
 * This is done automatically by the GameActivity: see `onKeyUp` and `onKeyDown`
 * to set a callback to consume the received events.
 * This function can be used if you re-implement events handling in your own
 * activity.
 * Ownership of out_event is maintained by the caller.
 */
void GameActivityKeyEvent_fromJava(JNIEnv* env, jobject motionEvent,
                                   GameActivityKeyEvent* out_event);

#ifdef __cplusplus
}
#endif

/** @} */

#endif // ANDROID_GAME_SDK_GAME_ACTIVITY_EVENTS_INTERNAL_H
