# ai-advent-challenge-02 - 03.11.2026

Install
```sh
python3 -m venv .venv
source .venv/bin/activate
pip install streamlit openai
```

Run
```sh
streamlit run 02/app.py
```

Result

|явное описание формата|max_tokens|stop_sequence|Json schema|Результат|
|-|-|-|-|-|
|-|-|-|-|Обычное поведение|
|в формате json|-|-|-|Ожидаемо выдаёт в формате json, но обычно с доп описанием|
|-|512|-|-|Останавливается по количеству токенов (как и ожидается), причем может остановиться на этапе reasoning|
|-|-|перец|-|Останавливается по стоп слову (как и ожидается), причем может остановиться на этапе reasoning|
|-|-|-|+|Отдаёт результат полностью в формте json, без доп описания. Json может быть не всегда валиден, как и ожидалось|

https://github.com/user-attachments/assets/4e7ced30-b1cb-4963-bf30-851844636158

https://github.com/user-attachments/assets/485be0d5-c1cf-4d5a-871b-ae6cd9d2a2c1

https://github.com/user-attachments/assets/2dbedaf6-fcd4-4c87-ad58-cd721ff0c22f
