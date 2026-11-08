# -*- coding: utf-8 -*-
"""
Эксперимент с параметром temperature (Streamlit + OpenAI).

Установка зависимостей:
    pip install streamlit openai

Запуск:
    streamlit run эксперимент_temperature.py
"""

import os
import json
import time
import datetime as dt
from collections import Counter

import streamlit as st
from openai import OpenAI

# ============================================================
# НАСТРОЙКИ — ВПИШИТЕ СВОИ ЗНАЧЕНИЯ ЗДЕСЬ
# ============================================================
API_KEY = os.getenv("DEEPSEEK_API_KEY")
BASE_URL = "https://api.deepseek.com"         # или адрес вашего прокси / совместимого API
MODEL = "deepseek-flash"                          # модель
RESULTS_DIR = "04/results"                        # папка для файлов с результатами
# ============================================================

st.set_page_config(page_title="Эксперимент: temperature", layout="wide")
st.title("Эксперимент: влияние temperature на ответы модели")


# ------------------------------------------------------------
# Вспомогательные функции
# ------------------------------------------------------------
def call_model(client: OpenAI, prompt: str, temperature: float) -> dict:
    """Один запрос к модели. Возвращает полный отчёт о запросе/ответе."""
    request_payload = {
        "model": MODEL,
        "temperature": temperature,
        "messages": [{"role": "user", "content": prompt}],
    }
    started = time.time()
    try:
        resp = client.chat.completions.create(**request_payload)
        latency = time.time() - started
        raw = resp.model_dump()
        return {
            "ok": True,
            "http_status": "200 OK",
            "request": request_payload,
            "answer": resp.choices[0].message.content,
            "finish_reason": resp.choices[0].finish_reason,
            "usage": raw.get("usage"),
            "latency_sec": round(latency, 3),
            "raw_response": raw,
            "error": None,
        }
    except Exception as e:  # сюда попадают и HTTP-ошибки API
        return {
            "ok": False,
            "http_status": "ERROR",
            "request": request_payload,
            "answer": None,
            "finish_reason": None,
            "usage": None,
            "latency_sec": round(time.time() - started, 3),
            "raw_response": None,
            "error": f"{type(e).__name__}: {e}",
        }


def analyze(records: list, expected: str = "") -> dict:
    """Метрики по набору прогонов одной комбинации."""
    answers = [(r["answer"] or "") for r in records if r["ok"]]
    normalized = [a.strip().lower() for a in answers]
    counts = Counter(normalized)
    most_common_answer, most_common_n = counts.most_common(1)[0] if counts else ("", 0)

    errors = sum(1 for r in records if not r["ok"])
    empty = sum(1 for r in records if r["ok"] and not (r["answer"] or "").strip())
    truncated = sum(1 for r in records if r["ok"] and r["finish_reason"] == "length")

    expected = (expected or "").strip().lower()
    correct = None
    if expected:
        correct = sum(1 for a in normalized if expected in a)

    return {
        "runs": len(records),
        "errors": errors,                       # запрос завершился ошибкой
        "empty_answers": empty,                 # «сломался формат»: пустой ответ
        "truncated": truncated,                 # ответ обрезан по max_tokens
        "unique_answers": len(set(normalized)), # сколько различных ответов
        "most_common_share": round(most_common_n / len(records), 3) if records else 0,
        "correct": correct,                     # сколько раз совпало с ожидаемым (если задан)
        "avg_latency_sec": round(sum(r["latency_sec"] for r in records) / len(records), 3) if records else 0,
    }


def build_conclusions(results: list, temps: list, runs: int) -> str:
    """Автоматические выводы по итогам всех комбинаций."""
    lines = ["## Выводы", ""]

    for task_label in ("A", "B"):
        subset = [r for r in results if r["task_label"] == task_label]
        if not subset:
            continue
        title = "Задача A (один правильный ответ)" if task_label == "A" else "Задача B (генерация)"
        lines.append(f"### {title}")
        for r in subset:
            m = r["metrics"]
            line = (
                f"- **temperature = {r['temperature']}**: уникальных ответов {m['unique_answers']} из {m['runs']}, "
                f"ошибок {m['errors']}, пустых {m['empty_answers']}, обрезанных {m['truncated']}"
            )
            if m["correct"] is not None:
                line += f", совпало с ожидаемым ответом: {m['correct']} из {m['runs']}"
            lines.append(line)
        lines.append("")

    # даёт ли temperature=0 одинаковый ответ
    zero = [r for r in results if abs(r["temperature"]) < 1e-9]
    lines.append("### Даёт ли temperature = 0 одинаковый ответ каждый раз?")
    for r in zero:
        m = r["metrics"]
        if m["unique_answers"] == 1 and m["runs"] > 0:
            verdict = "Да — все прогоны дали идентичный ответ."
        else:
            verdict = (
                f"Нет — получено {m['unique_answers']} различных ответа из {m['runs']}. "
                "temperature = 0 делает выбор токенов жадным, но не гарантирует "
                "побитовой воспроизводимости (недетерминизм железа/батчей на стороне API)."
            )
        lines.append(f"- Задача {r['task_label']}: {verdict}")
    lines.append("")

    lines.append("### Для каких задач подходит каждая настройка")
    lines.append(
        "- **temperature = 0** — задачи с одним правильным ответом: факты, вычисления, извлечение данных, "
        "классификация, строгий формат (JSON, таблицы). Максимальная стабильность и повторяемость."
    )
    lines.append(
        "- **temperature = 0.7** — универсальный баланс: связные, но не шаблонные тексты. "
        "Подходит для черновиков, писем, объяснений, диалогов."
    )
    lines.append(
        "- **temperature = 1.2** — мозговой штурм: идеи, варианты названий, креативные тексты. "
        "Разнообразие максимально, но растёт риск ошибок, битого формата и «галлюцинаций», "
        "поэтому для фактических задач вредна."
    )
    return "\n".join(lines)


# ------------------------------------------------------------
# Интерфейс: параметры эксперимента
# ------------------------------------------------------------
st.header("Параметры")

c1, c2, c3 = st.columns(3)
with c1:
    temp1 = st.number_input("Температура 1", value=0.0, step=0.1, format="%.2f", key="t1")
with c2:
    temp2 = st.number_input("Температура 2", value=0.7, step=0.1, format="%.2f", key="t2")
with c3:
    temp3 = st.number_input("Температура 3", value=1.2, step=0.1, format="%.2f", key="t3")

temps = [temp1, temp2, temp3]

task_a = st.text_area(
    "Задача A — с одним правильным ответом",
    value="Сколько будет 17 * 24? Ответь одним числом.",
    height=90,
)
expected_a = st.text_input(
    "Ожидаемый ответ для задачи A (необязательно — для автоподсчёта верных ответов)",
    value="408",
)
task_b = st.text_area(
    "Задача B — на генерацию (текст, идеи, варианты названий)",
    value="Придумай 5 названий для кофейни рядом с университетом.",
    height=90,
)
runs = st.number_input("Количество прогонов каждой комбинации", min_value=1, max_value=100, value=5, step=1)
runs = int(runs)

# ------------------------------------------------------------
# Список всех комбинаций
# ------------------------------------------------------------
combinations = []
for label, prompt, expected in (("A", task_a, expected_a), ("B", task_b, "")):
    for t in temps:
        combinations.append({"task_label": label, "prompt": prompt, "expected": expected, "temperature": t})

st.header(f"Комбинации для прогона ({len(combinations)} шт., всего {len(combinations) * runs} запросов)")
st.dataframe(
    [
        {
            "#": i + 1,
            "Задача": c["task_label"],
            "Промпт (начало)": (c["prompt"][:70] + "…") if len(c["prompt"]) > 70 else c["prompt"],
            "temperature": c["temperature"],
            "Прогонов": runs,
        }
        for i, c in enumerate(combinations)
    ],
    use_container_width=True,
)

# if API_KEY.startswith("ВСТАВЬТЕ"):
    # st.warning("Не забудьте вписать свой API_KEY (и при необходимости BASE_URL) в начале файла.")

# ------------------------------------------------------------
# Запуск
# ------------------------------------------------------------
run_clicked = st.button("Получить результат", type="primary", use_container_width=True)

if run_clicked:
    # if API_KEY.startswith("ВСТАВЬТЕ"):
        # st.error("Сначала впишите API_KEY в начале файла.")
        # st.stop()

    client = OpenAI(
        api_key=API_KEY,
        base_url=BASE_URL
    )

    session_ts = dt.datetime.now().strftime("%Y%m%d_%H%M%S")
    session_dir = os.path.join(RESULTS_DIR, session_ts)
    os.makedirs(session_dir, exist_ok=True)

    total = len(combinations) * runs
    done = 0
    log_lines = []

    progress = st.progress(0.0)
    status_text = st.empty()
    log_box = st.empty()
    started_all = time.time()

    all_results = []

    for ci, combo in enumerate(combinations, start=1):
        records = []
        for run_i in range(1, runs + 1):
            remaining = total - done
            status_text.info(
                f"Комбинация {ci}/{len(combinations)} (задача {combo['task_label']}, "
                f"temperature={combo['temperature']}) — прогон {run_i}/{runs}. "
                f"Осталось итераций: {remaining} из {total}."
            )
            rec = call_model(client, combo["prompt"], combo["temperature"])
            rec["run"] = run_i
            rec["timestamp"] = dt.datetime.now().isoformat(timespec="seconds")
            records.append(rec)
            done += 1

            preview = (rec["answer"] or rec["error"] or "").replace("\n", " ")[:100]
            log_lines.append(
                f"[{rec['timestamp']}] #{done}/{total} | задача {combo['task_label']} | "
                f"temp={combo['temperature']} | прогон {run_i} | {rec['http_status']} | "
                f"{rec['latency_sec']} c | {preview}"
            )
            progress.progress(done / total)
            log_box.code("\n".join(log_lines[-30:]), language="text")

        metrics = analyze(records, combo["expected"])
        combo_result = {
            "task_label": combo["task_label"],
            "prompt": combo["prompt"],
            "expected": combo["expected"],
            "temperature": combo["temperature"],
            "metrics": metrics,
            "records": records,
        }
        all_results.append(combo_result)

        # файл по каждой комбинации: запросы, ответы, сырые response, метрики
        fname = f"combo_{ci}_task{combo['task_label']}_temp{combo['temperature']}.json"
        with open(os.path.join(session_dir, fname), "w", encoding="utf-8") as f:
            json.dump(combo_result, f, ensure_ascii=False, indent=2)

    elapsed = round(time.time() - started_all, 1)
    conclusions = build_conclusions(all_results, temps, runs)
    summary = {
        "model": MODEL,
        "base_url": BASE_URL,
        "runs_per_combo": runs,
        "temperatures": temps,
        "total_requests": total,
        "elapsed_sec": elapsed,
        "combos": [
            {"task_label": r["task_label"], "prompt": r["prompt"], "temperature": r["temperature"], "metrics": r["metrics"]}
            for r in all_results
        ],
    }
    with open(os.path.join(session_dir, "summary.json"), "w", encoding="utf-8") as f:
        json.dump(summary, f, ensure_ascii=False, indent=2)
    with open(os.path.join(session_dir, "отчет.md"), "w", encoding="utf-8") as f:
        f.write(f"# Эксперимент с temperature\n\nМодель: {MODEL}\n\n{conclusions}\n")

    progress.progress(1.0)
    status_text.success(f"Готово за {elapsed} c. Выполнено запросов: {total}. Результаты сохранены в папку: {session_dir}")
    st.session_state["results"] = all_results
    st.session_state["conclusions"] = conclusions
    st.session_state["session_dir"] = session_dir

# ------------------------------------------------------------
# Итоги (показываются и после перерендера страницы)
# ------------------------------------------------------------
if "results" in st.session_state:
    all_results = st.session_state["results"]

    st.header("Сводная таблица метрик")
    st.dataframe(
        [
            {
                "Задача": r["task_label"],
                "temperature": r["temperature"],
                "Прогонов": r["metrics"]["runs"],
                "Уникальных ответов": r["metrics"]["unique_answers"],
                "Ошибок запроса": r["metrics"]["errors"],
                "Пустых (сломан формат)": r["metrics"]["empty_answers"],
                "Обрезано по токенам": r["metrics"]["truncated"],
                "Верных (если задан эталон)": r["metrics"]["correct"],
                "Ср. время, c": r["metrics"]["avg_latency_sec"],
            }
            for r in all_results
        ],
        use_container_width=True,
    )

    st.header("Ответы по каждой комбинации")
    for r in all_results:
        with st.expander(f"Задача {r['task_label']} — temperature = {r['temperature']}"):
            st.markdown(f"**Промпт:** {r['prompt']}")
            for rec in r["records"]:
                st.markdown(f"**Прогон {rec['run']}** — {rec['http_status']}, {rec['latency_sec']} c")
                if rec["ok"]:
                    st.text(rec["answer"])
                else:
                    st.error(rec["error"])

    st.header("Полный лог запросов (request / response)")
    for r in all_results:
        for rec in r["records"]:
            with st.expander(
                f"Задача {r['task_label']} | temp={r['temperature']} | прогон {rec['run']} | {rec['http_status']}"
            ):
                st.markdown("**Request:**")
                st.json(rec["request"])
                st.markdown("**Response:**")
                if rec["raw_response"]:
                    st.json(rec["raw_response"])
                else:
                    st.error(rec["error"])

    st.markdown(st.session_state["conclusions"])
    st.caption(f"Файлы с полными результатами: {st.session_state['session_dir']}")
