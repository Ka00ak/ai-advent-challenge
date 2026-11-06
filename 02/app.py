import os
import streamlit as st
from openai import OpenAI

# Настройка страницы
st.set_page_config(page_title="Kimi API Test", page_icon="🤖")
st.title("🤖 Простой запрос к Kimi API")

# Поле для ввода API-ключа (вставь свой ключ здесь)

api_key = os.getenv("KIMI_API_KEY")

# Текстовое поле для вопроса
user_input = st.text_area("Введите ваш запрос:", height=150, placeholder="Например: Привет, как дела?")

# Кнопка отправки
if st.button("Отправить", type="primary"):
    if not user_input.strip():
        st.warning("Пожалуйста, введи текст запроса.")
    else:
        # Инициализация клиента Kimi (совместим с OpenAI SDK)
        # Официальный base_url для Kimi API
        client = OpenAI(
            api_key=api_key,
            base_url="https://api.moonshot.ai/v1",
        )

        with st.spinner("Kimi думает..."):
            try:
                # Отправка запроса
                completion = client.chat.completions.create(
                    model="kimi-k3",
                    messages=[
                        {"role": "system", "content": "Ты полезный ассистент."},
                        {"role": "user", "content": user_input}
                    ],
                    temperature=1,
                )
                
                # Вывод результата
                st.success("Ответ получен:")
                st.markdown(completion.choices[0].message.content)

            except Exception as e:
                st.error(f"Ошибка при запросе к API: {e}")