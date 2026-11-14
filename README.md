# ai-advent-challenge

Лабораторные работы AI Advent Challenge.

| # | Дата | Стек | Запуск |
|---|------|------|--------|
| [01](./01) | 02.11.2026 | Python, Streamlit | `streamlit run 01/app.py` |
| [02](./02) | 03.11.2026 | Python, Streamlit | `streamlit run 02/app.py` |
| [03](./03) | 04.11.2026 | Python, Streamlit | `streamlit run 03/app.py` |
| [04](./04) | 05.11.2026 | Python, Streamlit | `streamlit run 04/app.py` |
| [05](./05) | 06.11.2026 | Python, Streamlit | `streamlit run 05/app.py` |
| [06](./06) | 14.11.2026 | Vite, Vue, TypeScript | `npm run dev -w 06` |

## Веб-лабы (06+)

Лабы с Vite управляются через npm workspaces из корня репозитория:

```sh
nvm use          # Node 24, см. .nvmrc
npm install      # один раз для всех веб-лаб
npm run dev -w 06
npm run build    # собрать все веб-лабы
```

Не коммить `node_modules/` и `dist/`, ключи API не хранить в `VITE_*`-переменных.
