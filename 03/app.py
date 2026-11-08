import os
import json
import time
import threading
import streamlit as st
from openai import OpenAI

st.set_page_config(page_title="Kimi API Test", page_icon="🤖")
st.title("🤖 Простой запрос к Kimi API")

api_key = os.getenv("KIMI_API_KEY")

user_input = st.text_area("Введите ваш запрос:", height=150, placeholder="Например: Привет, как дела?")

with st.expander("⚙️ Параметры генерации", expanded=True):
    # ===== НОВОЕ: выбор уровня рассуждения =====
    reasoning_effort = st.radio(
        "Уровень рассуждения (reasoning_effort)",
        options=["low", "high", "max"],
        index=1,
        horizontal=True,
        help="low — минимальное рассуждение (быстрее/дешевле); max — максимальное (глубокое, по умолчанию в K3)."
    )

    max_tokens_input = st.text_input(
        "Максимальное количество токенов (необязательно)",
        value="",
        placeholder="Пусто = значение по умолчанию модели",
        help="Оставь пустым, чтобы не передавать max_tokens. Допустимо от 1 до 8192."
    )
    max_tokens = None
    if max_tokens_input.strip():
        try:
            max_tokens = int(max_tokens_input.strip())
            if not (1 <= max_tokens <= 8192):
                st.warning("max_tokens должен быть в диапазоне 1–8192. Значение будет проигнорировано.")
                max_tokens = None
        except ValueError:
            st.warning("max_tokens должен быть целым числом. Значение будет проигнорировано.")
            max_tokens = None

    stop_input = st.text_input(
        "Stop sequence",
        placeholder="Например: END, ###",
        help="Последовательности, при встрече которых генерация остановится. Несколько — через запятую."
    )
    stop_sequences = [s.strip() for s in stop_input.split(",") if s.strip()] or None

    json_output = st.checkbox(
        "Structured output (JSON)",
        value=False,
        help="Заставляет модель вернуть валидный JSON. Обязательно укажи формат JSON в запросе!"
    )

client = OpenAI(
    api_key=api_key,
    base_url="https://api.moonshot.ai/v1",
)

if st.button("Отправить", type="primary"):
    if not user_input.strip():
        st.warning("Пожалуйста, введи текст запроса.")
    else:
        with st.spinner("Kimi думает..."):
            try:
                system_content = "Ты полезный ассистент."
                if json_output:
                    system_content += " Отвечай строго в формате JSON."

                request_params = {
                    "model": "kimi-k3",
                    "messages": [
                        {"role": "system", "content": system_content},
                        {"role": "user", "content": user_input}
                    ],
                    # ===== НОВОЕ: передаём reasoning_effort =====
                    "reasoning_effort": reasoning_effort,
                    # temperature НЕ передаём: для K3 он фиксирован
                }

                if max_tokens is not None:
                    request_params["max_tokens"] = max_tokens

                if stop_sequences:
                    request_params["stop"] = stop_sequences

                if json_output:
                    request_params["response_format"] = {"type": "json_object"}

                st.subheader("📤 Переданный контекст")
                with st.expander("Полный запрос (JSON)", expanded=False):
                    st.code(
                        json.dumps(request_params, indent=4, ensure_ascii=False),
                        language="json"
                    )

                st.subheader("⏱️ Время ответа")
                timer_placeholder = st.empty()

                result_holder = {}
                error_holder = {}
                done_event = threading.Event()

                def do_request():
                    try:
                        start = time.perf_counter()
                        raw = client.chat.completions.with_raw_response.create(**request_params)
                        result_holder["raw_response"] = raw
                        result_holder["elapsed"] = time.perf_counter() - start
                    except Exception as ex:
                        error_holder["error"] = ex
                    finally:
                        done_event.set()

                start_time = time.perf_counter()
                thread = threading.Thread(target=do_request, daemon=True)
                thread.start()

                while not done_event.is_set():
                    elapsed_live = time.perf_counter() - start_time
                    timer_placeholder.markdown(f"**⏳ Идёт запрос: {elapsed_live:.1f} с**")
                    time.sleep(0.1)

                total_elapsed = time.perf_counter() - start_time
                timer_placeholder.markdown(f"**✅ Готово за {total_elapsed:.3f} с**")

                if "error" in error_holder:
                    raise error_holder["error"]

                raw_response = result_holder["raw_response"]
                elapsed = result_holder["elapsed"]
                http_response = raw_response.http_response
                completion = raw_response.parse()

                st.subheader("🌐 Сырой HTTP-ответ")
                with st.expander("Сырое тело ответа (raw body)"):
                    try:
                        parsed = json.loads(http_response.text)
                    except json.JSONDecodeError:
                        parsed = http_response.text
                    formatted = json.dumps(parsed, indent=4, ensure_ascii=False)
                    st.code(formatted, language="json")

                choice = completion.choices[0]
                content = choice.message.content or ""

                # ===== НОВОЕ: показываем reasoning_content, если есть =====
                reasoning_content = getattr(choice.message, "reasoning_content", None)
                if reasoning_content:
                    with st.expander("🧠 Рассуждения модели (reasoning_content)", expanded=False):
                        st.markdown(reasoning_content)

                if choice.finish_reason == "length":
                    st.warning("⚠️ Генерация обрезана по лимиту max_tokens — ответ может быть неполным.")

                st.subheader("💬 Ответ")
                if json_output:
                    try:
                        parsed = json.loads(content)
                        st.success("✅ JSON корректен (валиден)")
                        st.json(parsed)
                    except json.JSONDecodeError as e:
                        st.error("❌ JSON некорректен (невалиден)")
                        st.code(
                            f"Ошибка: {e.msg}\nСтрока: {e.lineno}, колонка: {e.colno} (символ {e.pos})",
                            language="text"
                        )
                        lines = content.splitlines()
                        if 0 < e.lineno <= len(lines):
                            bad_line = lines[e.lineno - 1]
                            st.code(f"{bad_line}\n{' ' * (e.colno - 1)}^", language="text")
                else:
                    st.markdown(content)

            except Exception as e:
                st.error(f"Ошибка при запросе к API: {e}")