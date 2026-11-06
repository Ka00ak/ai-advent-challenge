import os
import json
import streamlit as st
from openai import OpenAI

# Настройка страницы
st.set_page_config(page_title="Kimi API Test", page_icon="🤖")
st.title("🤖 Простой запрос к Kimi API")

# Поле для ввода API-ключа (вставь свой ключ здесь)
api_key = os.getenv("KIMI_API_KEY")

# Текстовое поле для вопроса
user_input = st.text_area("Введите ваш запрос:", height=150, placeholder="Например: Привет, как дела?")

# Параметры генерации
with st.expander("⚙️ Параметры генерации", expanded=True):
    # Максимальное количество токенов
    max_tokens = st.number_input(
        "Максимальное количество токенов",
        min_value=1,
        max_value=8192,
        value=1024,
        step=64,
        help="Ограничивает длину ответа модели."
    )

    # Stop sequence (можно несколько через запятую)
    stop_input = st.text_input(
        "Stop sequence",
        placeholder="Например: END, ###",
        help="Последовательности, при встрече которых генерация остановится. Несколько — через запятую."
    )
    stop_sequences = [s.strip() for s in stop_input.split(",") if s.strip()] or None

    # Structured output (JSON или обычный текст)
    json_output = st.checkbox(
        "Structured output (JSON)",
        value=False,
        help="Заставляет модель вернуть валидный JSON. Обязательно укажи формат JSON в запросе!"
    )

# Инициализация клиента Kimi (совместим с OpenAI SDK)
client = OpenAI(
    api_key=api_key,
    base_url="https://api.moonshot.ai/v1",
)

# Кнопка отправки
if st.button("Отправить", type="primary"):
    if not user_input.strip():
        st.warning("Пожалуйста, введи текст запроса.")
    else:
        with st.spinner("Kimi думает..."):
            try:
                # Формируем системный промпт (для JSON добавляем инструкцию)
                system_content = "Ты полезный ассистент."
                if json_output:
                    system_content += " Отвечай строго в формате JSON."

                # Собираем параметры запроса
                request_params = {
                    "model": "kimi-k3",
                    "messages": [
                        {"role": "system", "content": system_content},
                        {"role": "user", "content": user_input}
                    ],
                    "temperature": 1,
                    "max_tokens": max_tokens,
                }

                if stop_sequences:
                    request_params["stop"] = stop_sequences

                if json_output:
                    request_params["response_format"] = {"type": "json_object"}

                # Отправка запроса
                completion = client.chat.completions.create(**request_params)

                # Вывод результата
                st.success("Ответ получен:")
                content = completion.choices[0].message.content

                if json_output:
                    # Красиво отображаем JSON
                    try:
                        st.json(json.loads(content))
                    except json.JSONDecodeError:
                        st.code(content, language="json")
                else:
                    st.markdown(content)

            except Exception as e:
                st.error(f"Ошибка при запросе к API: {e}")