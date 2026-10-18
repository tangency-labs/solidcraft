/*
 * Copyright (C) 2021 The Android Open Source Project
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
 * @defgroup game_text_input Game Text Input
 * The interface to use GameTextInput.
 * @{
 */

#pragma once

#include <android/rect.h>
#include <jni.h>
#include <stdint.h>

#include "common/gamesdk_common.h"

#ifdef __cplusplus
extern "C" {
#endif

#define GAMETEXTINPUT_VERSION_REVISION 7cd950d0022d01f1e7e2b470aba5a7b1abacdfaa
#define GAMETEXTINPUT_MAJOR_VERSION 4
#define GAMETEXTINPUT_MINOR_VERSION 3
#define GAMETEXTINPUT_BUGFIX_VERSION 0
#define GAMETEXTINPUT_PACKED_VERSION                                                         \
    ANDROID_GAMESDK_PACKED_VERSION(GAMETEXTINPUT_MAJOR_VERSION, GAMETEXTINPUT_MINOR_VERSION, \
                                   GAMETEXTINPUT_BUGFIX_VERSION)

/**
 * This struct holds a span within a region of text from start (inclusive) to
 * end (exclusive). An empty span or cursor position is specified with
 * start==end. An undefined span is specified with start = end = SPAN_UNDEFINED.
 */
typedef struct GameTextInputSpan {
    /** The start of the region (inclusive). */
    int32_t start;
    /** The end of the region (exclusive). */
    int32_t end;
} GameTextInputSpan;

/**
 * Values with special meaning in a GameTextInputSpan.
 */
enum GameTextInputSpanFlag : int32_t { SPAN_UNDEFINED = -1 };

/**
 * This struct holds the state of an editable section of text.
 * The text can have a selection and a composing region defined on it.
 * A composing region is used by IMEs that allow input using multiple steps to
 * compose a glyph or word. Use functions GameTextInput_getState and
 * GameTextInput_setState to read and modify the state that an IME is editing.
 */
typedef struct GameTextInputState {
    /**
     * Text owned by the state, as a modified UTF-8 string. Null-terminated.
     * https://en.wikipedia.org/wiki/UTF-8#Modified_UTF-8
     */
    const char* text_UTF8;
    /**
     * Length in bytes of text_UTF8, *not* including the null at end.
     */
    int32_t text_length;
    /**
     * A selection defined on the text.
     */
    GameTextInputSpan selection;
    /**
     * A composing region defined on the text.
     */
    GameTextInputSpan composingRegion;
} GameTextInputState;

/**
 * A callback called by GameTextInput_getState.
 * @param context User-defined context.
 * @param state State, owned by the library, that will be valid for the duration
 * of the callback.
 */
typedef void (*GameTextInputGetStateCallback)(void* context,
                                              const struct GameTextInputState* state);

/**
 * Opaque handle to the GameTextInput API.
 */
typedef struct GameTextInput GameTextInput;

/**
 * Initialize the GameTextInput library.
 * If called twice without GameTextInput_destroy being called, the same pointer
 * will be returned and a warning will be issued.
 * @param env A JNI env valid on the calling thread. All other calls to the resulting GameTextInput
 * object must be done on the same calling thread.
 * @param max_string_size The maximum length of a string that can be edited. If
 * zero, the maximum defaults to 65536 bytes. A buffer of this size is allocated
 * at initialization.
 * @return A handle to the library.
 */
GameTextInput* GameTextInput_init(JNIEnv* env, uint32_t max_string_size);

/**
 * When using GameTextInput, you need to create a gametextinput.InputConnection
 * on the Java side and pass it using this function to the library, unless using
 * GameActivity in which case this will be done for you. See the GameActivity
 * source code or GameTextInput samples for examples of usage.
 * @param input A valid GameTextInput library handle.
 * @param inputConnection A gametextinput.InputConnection object.
 */
void GameTextInput_setInputConnection(GameTextInput* input, jobject inputConnection);

/**
 * Unless using GameActivity, it is required to call this function from your
 * Java gametextinput.Listener.stateChanged method to convert eventState and
 * trigger any event callbacks. When using GameActivity, this does not need to
 * be called as event processing is handled by the Activity.
 * @param input A valid GameTextInput library handle.
 * @param eventState A Java gametextinput.State object.
 */
void GameTextInput_processEvent(GameTextInput* input, jobject eventState);

/**
 * Free any resources owned by the GameTextInput library.
 * Any subsequent calls to the library will fail until GameTextInput_init is
 * called again.
 * @param input A valid GameTextInput library handle.
 */
void GameTextInput_destroy(GameTextInput* input);

/**
 * Flags to be passed to GameTextInput_showIme.
 */
enum ShowImeFlags : uint32_t {
    SHOW_IME_UNDEFINED = 0, // Default value.
    SHOW_IMPLICIT = 1,      // Indicates that the user has forced the input method open so it
                            // should not be closed until they explicitly do so.
    SHOW_FORCED = 2         // Indicates that this is an implicit request to show the
                            // input window, not as the result of a direct request by
                            // the user. The window may not be shown in this case.
};

/**
 * Show the IME. Calls InputMethodManager.showSoftInput().
 * @param input A valid GameTextInput library handle.
 * @param flags Defined in ShowImeFlags above. For more information see:
 * https://developer.android.com/reference/android/view/inputmethod/InputMethodManager
 */
void GameTextInput_showIme(GameTextInput* input, uint32_t flags);

/**
 * Flags to be passed to GameTextInput_hideIme.
 */
enum HideImeFlags : uint32_t {
    HIDE_IME_UNDEFINED = 0, // Default value.
    HIDE_IMPLICIT_ONLY = 1, // Indicates that the soft input window should only be hidden if it
                            // was not explicitly shown by the user.
    HIDE_NOT_ALWAYS = 2,    // Indicates that the soft input window should normally be hidden,
                            // unless it was originally shown with SHOW_FORCED.
};

/**
 * Hide the IME. Calls InputMethodManager.hideSoftInputFromWindow().
 * @param input A valid GameTextInput library handle.
 * @param flags Defined in HideImeFlags above. For more information see:
 * https://developer.android.com/reference/android/view/inputmethod/InputMethodManager
 */
void GameTextInput_hideIme(GameTextInput* input, uint32_t flags);

/**
 * Restarts the input method. Calls InputMethodManager.restartInput().
 * @param input A valid GameTextInput library handle.
 */
void GameTextInput_restartInput(GameTextInput* input);

/**
 * Call a callback with the current GameTextInput state, which may have been
 * modified by changes in the IME and calls to GameTextInput_setState. We use a
 * callback rather than returning the state in order to simplify ownership of
 * text_UTF8 strings. These strings are only valid during the calling of the
 * callback.
 * @param input A valid GameTextInput library handle.
 * @param callback A function that will be called with valid state.
 * @param context Context used by the callback.
 */
void GameTextInput_getState(GameTextInput* input, GameTextInputGetStateCallback callback,
                            void* context);

/**
 * Set the current GameTextInput state. This state is reflected to any active
 * IME.
 * @param input A valid GameTextInput library handle.
 * @param state The state to set. Ownership is maintained by the caller and must
 * remain valid for the duration of the call.
 */
void GameTextInput_setState(GameTextInput* input, const GameTextInputState* state);

/**
 * Type of the callback needed by GameTextInput_setEventCallback that will be
 * called every time the IME state changes.
 * @param context User-defined context set in GameTextInput_setEventCallback.
 * @param current_state Current IME state, owned by the library and valid during
 * the callback.
 */
typedef void (*GameTextInputEventCallback)(void* context, const GameTextInputState* current_state);

/**
 * Optionally set a callback to be called whenever the IME state changes.
 * Not necessary if you are using GameActivity, which handles these callbacks
 * for you.
 * @param input A valid GameTextInput library handle.
 * @param callback Called by the library when the IME state changes.
 * @param context Context passed as first argument to the callback.
 * <b>This function is deprecated. Don't perform any complex processing inside
 * the callback other than copying the state variable. Using any synchronization
 * primitives inside this callback may cause a deadlock.</b>
 */
void GameTextInput_setEventCallback(GameTextInput* input, GameTextInputEventCallback callback,
                                    void* context);

/**
 * Type of the callback needed by GameTextInput_setImeInsetsCallback that will
 * be called every time the IME window insets change.
 * @param context User-defined context set in
 * GameTextInput_setImeWIndowInsetsCallback.
 * @param current_insets Current IME insets, owned by the library and valid
 * during the callback.
 */
typedef void (*GameTextInputImeInsetsCallback)(void* context, const ARect* current_insets);

/**
 * Optionally set a callback to be called whenever the IME insets change.
 * Not necessary if you are using GameActivity, which handles these callbacks
 * for you.
 * @param input A valid GameTextInput library handle.
 * @param callback Called by the library when the IME insets change.
 * @param context Context passed as first argument to the callback.
 */
void GameTextInput_setImeInsetsCallback(GameTextInput* input,
                                        GameTextInputImeInsetsCallback callback, void* context);

/**
 * Get the current window insets for the IME.
 * @param input A valid GameTextInput library handle.
 * @param insets Filled with the current insets by this function.
 */
void GameTextInput_getImeInsets(const GameTextInput* input, ARect* insets);

/**
 * Unless using GameActivity, it is required to call this function from your
 * Java gametextinput.Listener.onImeInsetsChanged method to
 * trigger any event callbacks. When using GameActivity, this does not need to
 * be called as insets processing is handled by the Activity.
 * @param input A valid GameTextInput library handle.
 * @param eventState A Java gametextinput.State object.
 */
void GameTextInput_processImeInsets(GameTextInput* input, const ARect* insets);

/**
 * Convert a GameTextInputState struct to a Java gametextinput.State object.
 * Don't forget to delete the returned Java local ref when you're done.
 * @param input A valid GameTextInput library handle.
 * @param state Input state to convert.
 * @return A Java object of class gametextinput.State. The caller is required to
 * delete this local reference.
 */
jobject GameTextInputState_toJava(const GameTextInput* input, const GameTextInputState* state);

/**
 * Convert from a Java gametextinput.State object into a C GameTextInputState
 * struct.
 * @param input A valid GameTextInput library handle.
 * @param state A Java gametextinput.State object.
 * @param callback A function called with the C struct, valid for the duration
 * of the call.
 * @param context Context passed to the callback.
 */
void GameTextInputState_fromJava(const GameTextInput* input, jobject state,
                                 GameTextInputGetStateCallback callback, void* context);

/**
 * Definitions for inputType argument of GameActivity_setImeEditorInfo()
 *
 * <pre>
 * |-------|-------|-------|-------|
 *                              1111 TYPE_MASK_CLASS
 *                      11111111     TYPE_MASK_VARIATION
 *          111111111111             TYPE_MASK_FLAGS
 * |-------|-------|-------|-------|
 *                                   TYPE_NULL
 * |-------|-------|-------|-------|
 *                                 1 TYPE_CLASS_TEXT
 *                             1     TYPE_TEXT_VARIATION_URI
 *                            1      TYPE_TEXT_VARIATION_EMAIL_ADDRESS
 *                            11     TYPE_TEXT_VARIATION_EMAIL_SUBJECT
 *                           1       TYPE_TEXT_VARIATION_SHORT_MESSAGE
 *                           1 1     TYPE_TEXT_VARIATION_LONG_MESSAGE
 *                           11      TYPE_TEXT_VARIATION_PERSON_NAME
 *                           111     TYPE_TEXT_VARIATION_POSTAL_ADDRESS
 *                          1        TYPE_TEXT_VARIATION_PASSWORD
 *                          1  1     TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
 *                          1 1      TYPE_TEXT_VARIATION_WEB_EDIT_TEXT
 *                          1 11     TYPE_TEXT_VARIATION_FILTER
 *                          11       TYPE_TEXT_VARIATION_PHONETIC
 *                          11 1     TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS
 *                          111      TYPE_TEXT_VARIATION_WEB_PASSWORD
 *                     1             TYPE_TEXT_FLAG_CAP_CHARACTERS
 *                    1              TYPE_TEXT_FLAG_CAP_WORDS
 *                   1               TYPE_TEXT_FLAG_CAP_SENTENCES
 *                  1                TYPE_TEXT_FLAG_AUTO_CORRECT
 *                 1                 TYPE_TEXT_FLAG_AUTO_COMPLETE
 *                1                  TYPE_TEXT_FLAG_MULTI_LINE
 *               1                   TYPE_TEXT_FLAG_IME_MULTI_LINE
 *              1                    TYPE_TEXT_FLAG_NO_SUGGESTIONS
 *             1 TYPE_TEXT_FLAG_ENABLE_TEXT_CONVERSION_SUGGESTIONS
 * |-------|-------|-------|-------|
 *                                1  TYPE_CLASS_NUMBER
 *                             1     TYPE_NUMBER_VARIATION_PASSWORD
 *                     1             TYPE_NUMBER_FLAG_SIGNED
 *                    1              TYPE_NUMBER_FLAG_DECIMAL
 * |-------|-------|-------|-------|
 *                                11 TYPE_CLASS_PHONE
 * |-------|-------|-------|-------|
 *                               1   TYPE_CLASS_DATETIME
 *                             1     TYPE_DATETIME_VARIATION_DATE
 *                            1      TYPE_DATETIME_VARIATION_TIME
 * |-------|-------|-------|-------|</pre>
 */

enum GameTextInputType : uint32_t {
    /**
     * Mask of bits that determine the overall class
     * of text being given.  Currently supported classes are:
     * {@link #TYPE_CLASS_TEXT}, {@link #TYPE_CLASS_NUMBER},
     * {@link #TYPE_CLASS_PHONE}, {@link #TYPE_CLASS_DATETIME}.
     * <p>IME authors: If the class is not one you
     * understand, assume {@link #TYPE_CLASS_TEXT} with NO variation
     * or flags.<p>
     */
    TYPE_MASK_CLASS = 0x0000000f,

    /**
     * Mask of bits that determine the variation of
     * the base content class.
     */
    TYPE_MASK_VARIATION = 0x00000ff0,

    /**
     * Mask of bits that provide addition bit flags
     * of options.
     */
    TYPE_MASK_FLAGS = 0x00fff000,

    /**
     * Special content type for when no explicit type has been specified.
     * This should be interpreted to mean that the target input connection
     * is not rich, it can not process and show things like candidate text nor
     * retrieve the current text, so the input method will need to run in a
     * limited "generate key events" mode, if it supports it. Note that some
     * input methods may not support it, for example a voice-based input
     * method will likely not be able to generate key events even if this
     * flag is set.
     */
    TYPE_NULL = 0x00000000,

    // ----------------------------------------------------------------------

    /**
     * Class for normal text.  This class supports the following flags (only
     * one of which should be set):
     * {@link #TYPE_TEXT_FLAG_CAP_CHARACTERS},
     * {@link #TYPE_TEXT_FLAG_CAP_WORDS}, and.
     * {@link #TYPE_TEXT_FLAG_CAP_SENTENCES}.  It also supports the
     * following variations:
     * {@link #TYPE_TEXT_VARIATION_NORMAL}, and
     * {@link #TYPE_TEXT_VARIATION_URI}.  If you do not recognize the
     * variation, normal should be assumed.
     */
    TYPE_CLASS_TEXT = 0x00000001,

    /**
     * Flag for {@link #TYPE_CLASS_TEXT}: capitalize all characters.  Overrides
     * {@link #TYPE_TEXT_FLAG_CAP_WORDS} and
     * {@link #TYPE_TEXT_FLAG_CAP_SENTENCES}.  This value is explicitly defined
     * to be the same as {@link TextUtils#CAP_MODE_CHARACTERS}. Of course,
     * this only affects languages where there are upper-case and lower-case
     * letters.
     */
    TYPE_TEXT_FLAG_CAP_CHARACTERS = 0x00001000,

    /**
     * Flag for {@link #TYPE_CLASS_TEXT}: capitalize the first character of
     * every word.  Overrides {@link #TYPE_TEXT_FLAG_CAP_SENTENCES}.  This
     * value is explicitly defined
     * to be the same as {@link TextUtils#CAP_MODE_WORDS}. Of course,
     * this only affects languages where there are upper-case and lower-case
     * letters.
     */
    TYPE_TEXT_FLAG_CAP_WORDS = 0x00002000,

    /**
     * Flag for {@link #TYPE_CLASS_TEXT}: capitalize the first character of
     * each sentence.  This value is explicitly defined
     * to be the same as {@link TextUtils#CAP_MODE_SENTENCES}. For example
     * in English it means to capitalize after a period and a space (note that
     * other languages may have different characters for period, or not use
     * spaces, or use different grammatical rules). Of course, this only affects
     * languages where there are upper-case and lower-case letters.
     */
    TYPE_TEXT_FLAG_CAP_SENTENCES = 0x00004000,

    /**
     * Flag for {@link #TYPE_CLASS_TEXT}: the user is entering free-form
     * text that should have auto-correction applied to it. Without this flag,
     * the IME will not try to correct typos. You should always set this flag
     * unless you really expect users to type non-words in this field, for
     * example to choose a name for a character in a game.
     * Contrast this with {@link #TYPE_TEXT_FLAG_AUTO_COMPLETE} and
     * {@link #TYPE_TEXT_FLAG_NO_SUGGESTIONS}:
     * {@code TYPE_TEXT_FLAG_AUTO_CORRECT} means that the IME will try to
     * auto-correct typos as the user is typing, but does not define whether
     * the IME offers an interface to show suggestions.
     */
    TYPE_TEXT_FLAG_AUTO_CORRECT = 0x00008000,

    /**
     * Flag for {@link #TYPE_CLASS_TEXT}: the text editor (which means
     * the application) is performing auto-completion of the text being entered
     * based on its own semantics, which it will present to the user as they type.
     * This generally means that the input method should not be showing
     * candidates itself, but can expect the editor to supply its own
     * completions/candidates from
     * {@link android.view.inputmethod.InputMethodSession#displayCompletions
     * InputMethodSession.displayCompletions()} as a result of the editor calling
     * {@link android.view.inputmethod.InputMethodManager#displayCompletions
     * InputMethodManager.displayCompletions()}.
     * Note the contrast with {@link #TYPE_TEXT_FLAG_AUTO_CORRECT} and
     * {@link #TYPE_TEXT_FLAG_NO_SUGGESTIONS}:
     * {@code TYPE_TEXT_FLAG_AUTO_COMPLETE} means the editor should show an
     * interface for displaying suggestions, but instead of supplying its own
     * it will rely on the Editor to pass completions/corrections.
     */
    TYPE_TEXT_FLAG_AUTO_COMPLETE = 0x00010000,

    /**
     * Flag for {@link #TYPE_CLASS_TEXT}: multiple lines of text can be
     * entered into the field.  If this flag is not set, the text field
     * will be constrained to a single line. The IME may also choose not to
     * display an enter key when this flag is not set, as there should be no
     * need to create new lines.
     */
    TYPE_TEXT_FLAG_MULTI_LINE = 0x00020000,

    /**
     * Flag for {@link #TYPE_CLASS_TEXT}: the regular text view associated
     * with this should not be multi-line, but when a fullscreen input method
     * is providing text it should use multiple lines if it can.
     */
    TYPE_TEXT_FLAG_IME_MULTI_LINE = 0x00040000,

    /**
     * Flag for {@link #TYPE_CLASS_TEXT}: the input method does not need to
     * display any dictionary-based candidates. This is useful for text views that
     * do not contain words from the language and do not benefit from any
     * dictionary-based completions or corrections. It overrides the
     * {@link #TYPE_TEXT_FLAG_AUTO_CORRECT} value when set.
     * Please avoid using this unless you are certain this is what you want.
     * Many input methods need suggestions to work well, for example the ones
     * based on gesture typing. Consider clearing
     * {@link #TYPE_TEXT_FLAG_AUTO_CORRECT} instead if you just do not
     * want the IME to correct typos.
     * Note the contrast with {@link #TYPE_TEXT_FLAG_AUTO_CORRECT} and
     * {@link #TYPE_TEXT_FLAG_AUTO_COMPLETE}:
     * {@code TYPE_TEXT_FLAG_NO_SUGGESTIONS} means the IME does not need to
     * show an interface to display suggestions. Most IMEs will also take this to
     * mean they do not need to try to auto-correct what the user is typing.
     */
    TYPE_TEXT_FLAG_NO_SUGGESTIONS = 0x00080000,

    /**
     * Flag for {@link #TYPE_CLASS_TEXT}: Let the IME know the text conversion
     * suggestions are required by the application. Text conversion suggestion is
     * for the transliteration languages which has pronunciation characters and
     * target characters. When the user is typing the pronunciation charactes, the
     * IME could provide the possible target characters to the user. When this
     * flag is set, the IME should insert the text conversion suggestions through
     * {@link Builder#setTextConversionSuggestions(List)} and
     * the {@link TextAttribute} with initialized with the text conversion
     * suggestions is provided by the IME to the application. To receive the
     * additional information, the application needs to implement {@link
     * InputConnection#setComposingText(CharSequence, int, TextAttribute)},
     * {@link InputConnection#setComposingRegion(int, int, TextAttribute)}, and
     * {@link InputConnection#commitText(CharSequence, int, TextAttribute)}.
     */
    TYPE_TEXT_FLAG_ENABLE_TEXT_CONVERSION_SUGGESTIONS = 0x00100000,

    // ----------------------------------------------------------------------

    /**
     * Default variation of {@link #TYPE_CLASS_TEXT}: plain old normal text.
     */
    TYPE_TEXT_VARIATION_NORMAL = 0x00000000,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering a URI.
     */
    TYPE_TEXT_VARIATION_URI = 0x00000010,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering an e-mail address.
     */
    TYPE_TEXT_VARIATION_EMAIL_ADDRESS = 0x00000020,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering the subject line of
     * an e-mail.
     */
    TYPE_TEXT_VARIATION_EMAIL_SUBJECT = 0x00000030,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering a short, possibly informal
     * message such as an instant message or a text message.
     */
    TYPE_TEXT_VARIATION_SHORT_MESSAGE = 0x00000040,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering the content of a long,
     * possibly formal message such as the body of an e-mail.
     */
    TYPE_TEXT_VARIATION_LONG_MESSAGE = 0x00000050,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering the name of a person.
     */
    TYPE_TEXT_VARIATION_PERSON_NAME = 0x00000060,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering a postal mailing address.
     */
    TYPE_TEXT_VARIATION_POSTAL_ADDRESS = 0x00000070,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering a password.
     */
    TYPE_TEXT_VARIATION_PASSWORD = 0x00000080,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering a password, which should
     * be visible to the user.
     */
    TYPE_TEXT_VARIATION_VISIBLE_PASSWORD = 0x00000090,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering text inside of a web form.
     */
    TYPE_TEXT_VARIATION_WEB_EDIT_TEXT = 0x000000a0,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering text to filter contents
     * of a list etc.
     */
    TYPE_TEXT_VARIATION_FILTER = 0x000000b0,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering text for phonetic
     * pronunciation, such as a phonetic name field in contacts. This is mostly
     * useful for languages where one spelling may have several phonetic
     * readings, like Japanese.
     */
    TYPE_TEXT_VARIATION_PHONETIC = 0x000000c0,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering e-mail address inside
     * of a web form.  This was added in
     * {@link android.os.Build.VERSION_CODES#HONEYCOMB}.  An IME must target
     * this API version or later to see this input type; if it doesn't, a request
     * for this type will be seen as {@link #TYPE_TEXT_VARIATION_EMAIL_ADDRESS}
     * when passed through {@link
     * android.view.inputmethod.EditorInfo#makeCompatible(int)
     * EditorInfo.makeCompatible(int)}.
     */
    TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS = 0x000000d0,

    /**
     * Variation of {@link #TYPE_CLASS_TEXT}: entering password inside
     * of a web form.  This was added in
     * {@link android.os.Build.VERSION_CODES#HONEYCOMB}.  An IME must target
     * this API version or later to see this input type; if it doesn't, a request
     * for this type will be seen as {@link #TYPE_TEXT_VARIATION_PASSWORD}
     * when passed through {@link
     * android.view.inputmethod.EditorInfo#makeCompatible(int)
     * EditorInfo.makeCompatible(int)}.
     */
    TYPE_TEXT_VARIATION_WEB_PASSWORD = 0x000000e0,

    // ----------------------------------------------------------------------

    /**
     * Class for numeric text.  This class supports the following flags:
     * {@link #TYPE_NUMBER_FLAG_SIGNED} and
     * {@link #TYPE_NUMBER_FLAG_DECIMAL}.  It also supports the following
     * variations: {@link #TYPE_NUMBER_VARIATION_NORMAL} and
     * {@link #TYPE_NUMBER_VARIATION_PASSWORD}.
     * <p>IME authors: If you do not recognize
     * the variation, normal should be assumed.</p>
     */
    TYPE_CLASS_NUMBER = 0x00000002,

    /**
     * Flag of {@link #TYPE_CLASS_NUMBER}: the number is signed, allowing
     * a positive or negative sign at the start.
     */
    TYPE_NUMBER_FLAG_SIGNED = 0x00001000,

    /**
     * Flag of {@link #TYPE_CLASS_NUMBER}: the number is decimal, allowing
     * a decimal point to provide fractional values.
     */
    TYPE_NUMBER_FLAG_DECIMAL = 0x00002000,

    // ----------------------------------------------------------------------

    /**
     * Default variation of {@link #TYPE_CLASS_NUMBER}: plain normal
     * numeric text.  This was added in
     * {@link android.os.Build.VERSION_CODES#HONEYCOMB}.  An IME must target
     * this API version or later to see this input type; if it doesn't, a request
     * for this type will be dropped when passed through
     * {@link android.view.inputmethod.EditorInfo#makeCompatible(int)
     * EditorInfo.makeCompatible(int)}.
     */
    TYPE_NUMBER_VARIATION_NORMAL = 0x00000000,

    /**
     * Variation of {@link #TYPE_CLASS_NUMBER}: entering a numeric password.
     * This was added in {@link android.os.Build.VERSION_CODES#HONEYCOMB}.  An
     * IME must target this API version or later to see this input type; if it
     * doesn't, a request for this type will be dropped when passed
     * through {@link android.view.inputmethod.EditorInfo#makeCompatible(int)
     * EditorInfo.makeCompatible(int)}.
     */
    TYPE_NUMBER_VARIATION_PASSWORD = 0x00000010,

    // ----------------------------------------------------------------------
    /**
     * Class for a phone number.  This class currently supports no variations
     * or flags.
     */
    TYPE_CLASS_PHONE = 0x00000003,

    // ----------------------------------------------------------------------

    /**
     * Class for dates and times.  It supports the
     * following variations:
     * {@link #TYPE_DATETIME_VARIATION_NORMAL}
     * {@link #TYPE_DATETIME_VARIATION_DATE}, and
     * {@link #TYPE_DATETIME_VARIATION_TIME}.
     */
    TYPE_CLASS_DATETIME = 0x00000004,

    /**
     * Default variation of {@link #TYPE_CLASS_DATETIME}: allows entering
     * both a date and time.
     */
    TYPE_DATETIME_VARIATION_NORMAL = 0x00000000,

    /**
     * Default variation of {@link #TYPE_CLASS_DATETIME}: allows entering
     * only a date.
     */
    TYPE_DATETIME_VARIATION_DATE = 0x00000010,

    /**
     * Default variation of {@link #TYPE_CLASS_DATETIME}: allows entering
     * only a time.
     */
    TYPE_DATETIME_VARIATION_TIME = 0x00000020,
};

/**
 * actionId and imeOptions argument of GameActivity_setImeEditorInfo().
 *
 * <pre>
 * |-------|-------|-------|-------|
 *                              1111 IME_MASK_ACTION
 * |-------|-------|-------|-------|
 *                                   IME_ACTION_UNSPECIFIED
 *                                 1 IME_ACTION_NONE
 *                                1  IME_ACTION_GO
 *                                11 IME_ACTION_SEARCH
 *                               1   IME_ACTION_SEND
 *                               1 1 IME_ACTION_NEXT
 *                               11  IME_ACTION_DONE
 *                               111 IME_ACTION_PREVIOUS
 *         1                         IME_FLAG_NO_PERSONALIZED_LEARNING
 *        1                          IME_FLAG_NO_FULLSCREEN
 *       1                           IME_FLAG_NAVIGATE_PREVIOUS
 *      1                            IME_FLAG_NAVIGATE_NEXT
 *     1                             IME_FLAG_NO_EXTRACT_UI
 *    1                              IME_FLAG_NO_ACCESSORY_ACTION
 *   1                               IME_FLAG_NO_ENTER_ACTION
 *  1                                IME_FLAG_FORCE_ASCII
 * |-------|-------|-------|-------|</pre>
 */

enum GameTextInputActionType : uint32_t {
    /**
     * Set of bits in {@link #imeOptions} that provide alternative actions
     * associated with the "enter" key.  This both helps the IME provide
     * better feedback about what the enter key will do, and also allows it
     * to provide alternative mechanisms for providing that command.
     */
    IME_MASK_ACTION = 0x000000ff,

    /**
     * Bits of {@link #IME_MASK_ACTION}: no specific action has been
     * associated with this editor, let the editor come up with its own if
     * it can.
     */
    IME_ACTION_UNSPECIFIED = 0x00000000,

    /**
     * Bits of {@link #IME_MASK_ACTION}: there is no available action.
     */
    IME_ACTION_NONE = 0x00000001,

    /**
     * Bits of {@link #IME_MASK_ACTION}: the action key performs a "go"
     * operation to take the user to the target of the text they typed.
     * Typically used, for example, when entering a URL.
     */
    IME_ACTION_GO = 0x00000002,

    /**
     * Bits of {@link #IME_MASK_ACTION}: the action key performs a "search"
     * operation, taking the user to the results of searching for the text
     * they have typed (in whatever context is appropriate).
     */
    IME_ACTION_SEARCH = 0x00000003,

    /**
     * Bits of {@link #IME_MASK_ACTION}: the action key performs a "send"
     * operation, delivering the text to its target.  This is typically used
     * when composing a message in IM or SMS where sending is immediate.
     */
    IME_ACTION_SEND = 0x00000004,

    /**
     * Bits of {@link #IME_MASK_ACTION}: the action key performs a "next"
     * operation, taking the user to the next field that will accept text.
     */
    IME_ACTION_NEXT = 0x00000005,

    /**
     * Bits of {@link #IME_MASK_ACTION}: the action key performs a "done"
     * operation, typically meaning there is nothing more to input and the
     * IME will be closed.
     */
    IME_ACTION_DONE = 0x00000006,

    /**
     * Bits of {@link #IME_MASK_ACTION}: like {@link #IME_ACTION_NEXT}, but
     * for moving to the previous field.  This will normally not be used to
     * specify an action (since it precludes {@link #IME_ACTION_NEXT}), but
     * can be returned to the app if it sets {@link #IME_FLAG_NAVIGATE_PREVIOUS}.
     */
    IME_ACTION_PREVIOUS = 0x00000007,
};

enum GameTextInputImeOptions : uint32_t {
    /**
     * Flag of {@link #imeOptions}: used to request that the IME should not update
     * any personalized data such as typing history and personalized language
     * model based on what the user typed on this text editing object.  Typical
     * use cases are: <ul> <li>When the application is in a special mode, where
     * user's activities are expected to be not recorded in the application's
     * history.  Some web browsers and chat applications may have this kind of
     * modes.</li> <li>When storing typing history does not make much sense.
     * Specifying this flag in typing games may help to avoid typing history from
     * being filled up with words that the user is less likely to type in their
     * daily life.  Another example is that when the application already knows
     * that the expected input is not a valid word (e.g. a promotion code that is
     *     not a valid word in any natural language).</li>
     * </ul>
     *
     * <p>Applications need to be aware that the flag is not a guarantee, and some
     * IMEs may not respect it.</p>
     */
    IME_FLAG_NO_PERSONALIZED_LEARNING = 0x1000000,

    /**
     * Flag of {@link #imeOptions}: used to request that the IME never go
     * into fullscreen mode.
     * By default, IMEs may go into full screen mode when they think
     * it's appropriate, for example on small screens in landscape
     * orientation where displaying a software keyboard may occlude
     * such a large portion of the screen that the remaining part is
     * too small to meaningfully display the application UI.
     * If this flag is set, compliant IMEs will never go into full screen mode,
     * and always leave some space to display the application UI.
     * Applications need to be aware that the flag is not a guarantee, and
     * some IMEs may ignore it.
     */
    IME_FLAG_NO_FULLSCREEN = 0x2000000,

    /**
     * Flag of {@link #imeOptions}: like {@link #IME_FLAG_NAVIGATE_NEXT}, but
     * specifies there is something interesting that a backward navigation
     * can focus on.  If the user selects the IME's facility to backward
     * navigate, this will show up in the application as an {@link
     * #IME_ACTION_PREVIOUS} at {@link InputConnection#performEditorAction(int)
     * InputConnection.performEditorAction(int)}.
     */
    IME_FLAG_NAVIGATE_PREVIOUS = 0x4000000,

    /**
     * Flag of {@link #imeOptions}: used to specify that there is something
     * interesting that a forward navigation can focus on. This is like using
     * {@link #IME_ACTION_NEXT}, except allows the IME to be multiline (with
     * an enter key) as well as provide forward navigation.  Note that some
     * IMEs may not be able to do this, especially when running on a small
     * screen where there is little space.  In that case it does not need to
     * present a UI for this option.  Like {@link #IME_ACTION_NEXT}, if the
     * user selects the IME's facility to forward navigate, this will show up
     * in the application at {@link InputConnection#performEditorAction(int)
     * InputConnection.performEditorAction(int)}.
     */
    IME_FLAG_NAVIGATE_NEXT = 0x8000000,

    /**
     * Flag of {@link #imeOptions}: used to specify that the IME does not need
     * to show its extracted text UI.  For input methods that may be fullscreen,
     * often when in landscape mode, this allows them to be smaller and let part
     * of the application be shown behind, through transparent UI parts in the
     * fullscreen IME. The part of the UI visible to the user may not be
     * responsive to touch because the IME will receive touch events, which may
     * confuse the user; use {@link #IME_FLAG_NO_FULLSCREEN} instead for a better
     * experience. Using this flag is discouraged and it may become deprecated in
     * the future. Its meaning is unclear in some situations and it may not work
     * appropriately on older versions of the platform.
     */
    IME_FLAG_NO_EXTRACT_UI = 0x10000000,

    /**
     * Flag of {@link #imeOptions}: used in conjunction with one of the actions
     * masked by {@link #IME_MASK_ACTION}, this indicates that the action
     * should not be available as an accessory button on the right of the
     * extracted text when the input method is full-screen. Note that by setting
     * this flag, there can be cases where the action is simply never available to
     * the user. Setting this generally means that you think that in fullscreen
     * mode, where there is little space to show the text, it's not worth taking
     * some screen real estate to display the action and it should be used instead
     * to show more text.
     */
    IME_FLAG_NO_ACCESSORY_ACTION = 0x20000000,

    /**
     * Flag of {@link #imeOptions}: used in conjunction with one of the actions
     * masked by {@link #IME_MASK_ACTION}. If this flag is not set, IMEs will
     * normally replace the "enter" key with the action supplied. This flag
     * indicates that the action should not be available in-line as a replacement
     * for the "enter" key. Typically this is because the action has such a
     * significant impact or is not recoverable enough that accidentally hitting
     * it should be avoided, such as sending a message. Note that
     * {@link android.widget.TextView} will automatically set this flag for you
     * on multi-line text views.
     */
    IME_FLAG_NO_ENTER_ACTION = 0x40000000,

    /**
     * Flag of {@link #imeOptions}: used to request an IME that is capable of
     * inputting ASCII characters.  The intention of this flag is to ensure that
     * the user can type Roman alphabet characters in a {@link
     * android.widget.TextView}. It is typically used for an account ID or
     * password input. A lot of the time, IMEs are already able to input ASCII
     * even without being told so (such IMEs already respect this flag in a
     * sense), but there are cases when this is not the default. For instance,
     * users of languages using a different script like Arabic, Greek, Hebrew or
     * Russian typically have a keyboard that can't input ASCII characters by
     * default. Applications need to be aware that the flag is not a guarantee,
     * and some IMEs may not respect it. However, it is strongly recommended for
     * IME authors to respect this flag especially when their IME could end up
     * with a state where only languages using non-ASCII are enabled.
     */
    IME_FLAG_FORCE_ASCII = 0x80000000,

    /**
     * Flag of {@link #internalImeOptions}: flag is set when app window containing
     * this
     * {@link EditorInfo} is using {@link Configuration#ORIENTATION_PORTRAIT}
     * mode.
     * @hide
     */
    IME_INTERNAL_FLAG_APP_WINDOW_PORTRAIT = 0x00000001,

    /**
     * Generic unspecified type for {@link #imeOptions}.
     */
    IME_NULL = 0x00000000,
};

#ifdef __cplusplus
}
#endif

/** @} */
