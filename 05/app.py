# -*- coding: utf-8 -*-
"""
Streamlit-вебморда для сравнения 3 LLM-моделей на 8 задачах.

Запуск:
    pip install streamlit openai
    streamlit run llm_bench_streamlit.py

Что делает:
  - 3 выпадающих списка моделей (дешёвая / средняя / флагманская), список и ключи задаются в коде ниже.
  - 8 полей ввода задач.
  - Перед запуском показывает полный список предстоящих запросов (каждая задача x каждая модель).
  - Одна кнопка «Получить результат» — прогоняет все комбинации, показывает живой статус
    (запрос/ответ, время, токены, стоимость, сколько итераций осталось) и пишет всё в файлы.
  - В конце — итоговая таблица «модель -> верных ответов / время / токены / стоимость» + выводы.
"""

import os
import csv
import json
import time
from datetime import datetime

import streamlit as st
from openai import OpenAI

# =====================================================================
# НАСТРОЙКИ: список моделей, ключи, цены (редактируй под себя)
# =====================================================================
# Цены — в USD за 1 МЛН токенов (input / output). Ставь 0.0 для бесплатных.
MODEL_CATALOG = [
    {
        "label": "deepseek-flash",
        "model": "deepseek-flash",
        "api_key": os.getenv("DEEPSEEK_API_KEY"),
        "base_url": "https://api.deepseek.com",
        "price_input_per_1m": 0.15,
        "price_output_per_1m": 0.6
    },
    {
        "label": "kimi-k3",
        "model": "kimi-k3",
        "api_key": os.getenv("KIMI_API_KEY"),
        "base_url": "https://api.moonshot.ai/v1",
        "price_input_per_1m": 3.00,
        "price_output_per_1m": 15.00,
    },
    {
        "label": "kimi-k3.1.pro",
        "model": "kimi-k3.1-pro",
        "api_key": os.getenv("KIMI_API_KEY"),
        "base_url": "https://api.moonshot.ai/v1",
        "price_input_per_1m": 2.00,
        "price_output_per_1m": 10.00,
    },

    # Пример модели другого провайдера через OpenAI-совместимый API:
    # {
    #     "label": "deepseek-chat",
    #     "model": "deepseek-chat",
    #     "api_key": "sk-...КЛЮЧ_DEEPSEEK...",
    #     "base_url": "https://api.deepseek.com/v1",
    #     "price_input_per_1m": 0.27,
    #     "price_output_per_1m": 1.10,
    # },
]

N_TASKS = 8
# MAX_TOKENS = 2048
TEMPERATURE = 1.0
RUNS_DIR = "05/results"  # сюда пишутся файлы с результатами

# =====================================================================
# ИНТЕРФЕЙС
# =====================================================================
st.set_page_config(page_title="LLM Bench: 3 модели x 8 задач", layout="wide")
st.title("Сравнение LLM: дешёвая vs средняя vs флагманская")

labels = [m["label"] for m in MODEL_CATALOG]

st.subheader("1. Выбор моделей")
c1, c2, c3 = st.columns(3)
with c1:
    cheap_label = st.selectbox("Дешёвая модель", labels, index=0)
with c2:
    mid_label = st.selectbox("Средняя модель", labels, index=min(1, len(labels) - 1))
with c3:
    flag_label = st.selectbox("Флагманская модель", labels, index=min(2, len(labels) - 1))

selected_labels = [cheap_label, mid_label, flag_label]
selected_models = [MODEL_CATALOG[labels.index(l)] for l in selected_labels]

st.subheader("2. Задачи для прогона")
tasks = []
cols = st.columns(2)
for i in range(N_TASKS):
    with cols[i % 2]:
        tasks.append(st.text_area(f"Задача {i + 1}", key=f"task_{i}", height=100,
                                  placeholder=f"Впиши сюда задачу {i + 1}..."))

filled_tasks = [t.strip() for t in tasks if t.strip()]

# ---------------------------------------------------------------------
# Список предстоящих запросов
# ---------------------------------------------------------------------
st.subheader("3. Предстоящие запросы")
planned = [(mi, m, ti, t) for mi, m in enumerate(selected_models)
           for ti, t in enumerate(filled_tasks)]
st.write(f"Всего запросов: **{len(planned)}** "
         f"({len(selected_models)} модели x {len(filled_tasks)} задач)")
with st.expander("Показать полный список запросов", expanded=True):
    for n, (mi, m, ti, t) in enumerate(planned, start=1):
        preview = (t[:90] + "...") if len(t) > 90 else t
        st.markdown(f"`{n:02d}` | **{m['label']}** | задача {ti + 1}: {preview}")

# =====================================================================
# ЗАПУСК
# =====================================================================
st.subheader("4. Запуск и статус")
run_button = st.button("Получить результат", type="primary")

if run_button:
    if not filled_tasks:
        st.error("Впиши хотя бы одну задачу.")
        st.stop()
    if len(set(selected_labels)) != len(selected_labels):
        st.warning("Внимание: выбраны одинаковые модели — сравнение будет менее полезным.")

    run_id = datetime.now().strftime("%Y%m%d_%H%M%S")
    run_dir = os.path.join(RUNS_DIR, run_id)
    os.makedirs(run_dir, exist_ok=True)

    results = []
    total = len(planned)
    progress = st.progress(0.0, text="Старт...")
    stats_placeholder = st.empty()
    log_container = st.container()

    clients = {}
    for m in selected_models:
        clients[m["label"]] = OpenAI(api_key=m["api_key"], base_url=m["base_url"])

    t_start_all = time.time()

    for idx, (mi, m, ti, task_text) in enumerate(planned, start=1):
        label = m["label"]
        remaining = total - idx

        request_payload = {
            "model": m["model"],
            "messages": [{"role": "user", "content": task_text}],
            "temperature": TEMPERATURE,
        }

        with log_container.status(
                f"[{idx}/{total}] {label} | задача {ti + 1} — отправляю запрос...",
                expanded=(idx == 1)) as st_status:
            st.markdown("**HTTP request:**")
            st.code(f"POST {m['base_url']}/chat/completions\n" +
                    json.dumps(request_payload, ensure_ascii=False, indent=2),
                    language="json")

            t0 = time.time()
            err = None
            answer, prompt_tok, completion_tok = "", 0, 0
            http_status = "200 OK"
            try:
                resp = clients[label].chat.completions.create(**request_payload)
                elapsed = time.time() - t0
                answer = resp.choices[0].message.content or ""
                if resp.usage:
                    prompt_tok = resp.usage.prompt_tokens or 0
                    completion_tok = resp.usage.completion_tokens or 0
            except Exception as e:  # фиксируем, но не роняем прогон
                elapsed = time.time() - t0
                err = f"{type(e).__name__}: {e}"
                http_status = "ERROR"

            cost = (prompt_tok * m["price_input_per_1m"] +
                    completion_tok * m["price_output_per_1m"]) / 1_000_000

            rec = {
                "n": idx,
                "model_label": label,
                "model": m["model"],
                "task_n": ti + 1,
                "task": task_text,
                "elapsed_sec": round(elapsed, 3),
                "prompt_tokens": prompt_tok,
                "completion_tokens": completion_tok,
                "total_tokens": prompt_tok + completion_tok,
                "cost_usd": round(cost, 6),
                "http_status": http_status,
                "error": err,
                "answer": answer,
                "correct": None,  # заполняется ручной оценкой ниже
            }
            results.append(rec)

            st.markdown(f"**HTTP response:** `{http_status}` | "
                        f"время **{elapsed:.2f} с** | токены in/out: "
                        f"**{prompt_tok}/{completion_tok}** | стоимость **${cost:.6f}**")
            if err:
                st.error(err)
            else:
                st.markdown("**Ответ модели:**")
                st.markdown(answer)

            st_status.update(
                label=f"[{idx}/{total}] {label} | задача {ti + 1} — "
                      f"{'ОШИБКА' if err else 'готово'} ({elapsed:.2f} с)",
                state="error" if err else "complete",
                expanded=False)

        # пишем ответ в отдельный файл сразу — ничего не потеряется при обрыве
        safe = "".join(ch if ch.isalnum() else "_" for ch in m["model"])
        with open(os.path.join(run_dir, f"{idx:02d}_{safe}_task{ti + 1}.md"),
                  "w", encoding="utf-8") as f:
            f.write(f"# {label} — задача {ti + 1}\n\n")
            f.write(f"**Задача:**\n{task_text}\n\n**Ответ:**\n{answer}\n\n")
            f.write(f"---\nВремя: {elapsed:.3f} с | Токены: {prompt_tok}+{completion_tok} | "
                    f"Стоимость: ${cost:.6f} | Статус: {http_status}\n")
            if err:
                f.write(f"Ошибка: {err}\n")

        progress.progress(idx / total,
                          text=f"Выполнено {idx}/{total} • осталось {remaining}")
        done_cost = sum(r["cost_usd"] for r in results)
        done_time = sum(r["elapsed_sec"] for r in results)
        stats_placeholder.info(
            f"Осталось итераций: **{remaining}** из {total} | "
            f"прошло: {time.time() - t_start_all:.1f} с | "
            f"суммарное время API-ответов: {done_time:.1f} с | "
            f"потрачено: ${done_cost:.4f}")

    progress.progress(1.0, text="Прогон завершён")
    st.success(f"Готово: {total} запросов за {time.time() - t_start_all:.1f} с. "
               f"Файлы сохранены в `{run_dir}/`")

    # файлы результатов
    with open(os.path.join(run_dir, "results.json"), "w", encoding="utf-8") as f:
        json.dump({"run_id": run_id,
                   "models": [{k: v for k, v in m.items() if k != "api_key"}
                              for m in selected_models],
                   "results": results}, f, ensure_ascii=False, indent=2)
    with open(os.path.join(run_dir, "results.csv"), "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=list(results[0].keys()))
        w.writeheader()
        w.writerows(results)

    st.session_state["results"] = results
    st.session_state["run_dir"] = run_dir
    st.session_state["models"] = [m["label"] for m in selected_models]

# =====================================================================
# РУЧНАЯ ОЦЕНКА + ИТОГОВАЯ ТАБЛИЦА
# =====================================================================
if "results" in st.session_state:
    results = st.session_state["results"]
    model_labels = st.session_state["models"]

    st.divider()
    st.subheader("5. Оценка ответов")
    st.caption("Проставь галочки «верно» — итоговая таблица пересчитается автоматически.")
    for i, r in enumerate(results):
        with st.expander(f"#{r['n']} | {r['model_label']} | задача {r['task_n']} "
                         f"({r['elapsed_sec']:.2f} с, {r['total_tokens']} ток., "
                         f"${r['cost_usd']:.6f})"):
            st.markdown(f"**Задача:** {r['task']}")
            st.markdown(f"**Ответ:**\n{r['answer'] if not r['error'] else '⚠️ ' + r['error']}")
            checked = st.checkbox("Ответ верный", key=f"ok_{i}",
                                  value=bool(r["correct"]))
            r["correct"] = checked
            if r["correct"]:
                # обновляем JSON с оценками
                pass

    # итоговая таблица
    st.subheader("6. Итоговая таблица")
    table = {}
    for lbl in model_labels:
        rows = [r for r in results if r["model_label"] == lbl]
        n_ok = sum(1 for r in rows if r["correct"])
        t_sum = sum(r["elapsed_sec"] for r in rows)
        tok_sum = sum(r["total_tokens"] for r in rows)
        cost_sum = sum(r["cost_usd"] for r in rows)
        errs = sum(1 for r in rows if r["error"])
        table[lbl] = {
            "Верных ответов": f"{n_ok}/{len(rows)}",
            "Суммарное время, с": round(t_sum, 2),
            "Среднее время, с": round(t_sum / max(len(rows), 1), 2),
            "Токенов всего": tok_sum,
            "Стоимость, $": round(cost_sum, 6),
            "Ошибок API": errs,
        }
    st.table(table)

    # сравнение флагмана с дешёвой
    if len(model_labels) >= 2:
        cheap, flag = model_labels[0], model_labels[-1]
        c, f = table[cheap], table[flag]
        cost_ratio = (f["Стоимость, $"] / c["Стоимость, $"]) if c["Стоимость, $"] > 0 else float("inf")
        time_ratio = (f["Среднее время, с"] / c["Среднее время, с"]) if c["Среднее время, с"] > 0 else float("inf")
        ok_c = int(c["Верных ответов"].split("/")[0])
        ok_f = int(f["Верных ответов"].split("/")[0])
        st.subheader("7. Выводы")
        st.markdown(f"""
- **Во сколько раз флагман дороже дешёвой модели:** {f"{cost_ratio:.1f}x" if cost_ratio != float("inf") else "∞ (дешёвая бесплатна)"}
- **Разница в точности:** флагман решил верно {ok_f}, дешёвая — {ok_c} (разница {ok_f - ok_c:+d})
- **Скорость:** среднее время флагмана {f['Среднее время, с']} с против {c['Среднее время, с']} с у дешёвой ({f'{time_ratio:.1f}x' if time_ratio != float('inf') else '—'})
- **Ресурсоёмкость (токены):** см. колонку «Токенов всего» — обычно флагманы пишут развёрнутее.
- **На каких задачах достаточно маленькой модели:** на тех, где у дешёвой модели стоит галочка «верно» — там платить за флагман нет смысла; флагман оправдан только на задачах, которые дешёвая решила неверно.
""")

    # обновляем файлы с оценками
    run_dir = st.session_state["run_dir"]
    with open(os.path.join(run_dir, "results.json"), "w", encoding="utf-8") as fj:
        json.dump({"results": results}, fj, ensure_ascii=False, indent=2)
    with open(os.path.join(run_dir, "summary.csv"), "w", newline="", encoding="utf-8") as fs:
        w = csv.writer(fs)
        w.writerow(["Модель", "Верных ответов", "Суммарное время, с", "Среднее время, с",
                    "Токенов всего", "Стоимость, $", "Ошибок API"])
        for lbl, row in table.items():
            w.writerow([lbl, row["Верных ответов"], row["Суммарное время, с"],
                        row["Среднее время, с"], row["Токенов всего"],
                        row["Стоимость, $"], row["Ошибок API"]])
