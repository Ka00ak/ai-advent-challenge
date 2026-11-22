//! 18 probe-вопросов (6 STM + 6 WM + 6 LTM) и keyword-оценка.

use serde::{Deserialize, Serialize};

use crate::domain::memory::MemoryLayer;
use crate::memory::retrieval::normalize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Probe {
    pub id: String,
    pub layer: MemoryLayer,
    pub question: String,
    pub keywords: Vec<String>,
    pub expected: String,
    pub quick: bool,
}

fn probe(
    id: &str,
    layer: MemoryLayer,
    question: &str,
    keywords: &[&str],
    expected: &str,
    quick: bool,
) -> Probe {
    Probe {
        id: id.to_string(),
        layer,
        question: question.to_string(),
        keywords: keywords.iter().map(|k| (*k).to_string()).collect(),
        expected: expected.to_string(),
        quick,
    }
}

pub fn all_probes() -> Vec<Probe> {
    use MemoryLayer::{Ltm, Stm, Wm};
    vec![
        probe(
            "S1",
            Stm,
            "До какой суммы подняли бюджет маркетинга?",
            &["150 000", "150000", "150 тысяч", "150 тыс"],
            "150 000",
            true,
        ),
        probe(
            "S2",
            Stm,
            "Что решили насчёт инфлюенсеров?",
            &["инфлюенсер", "25 000"],
            "отдельной строкой",
            false,
        ),
        probe(
            "S3",
            Stm,
            "Когда созвон с заказчиком?",
            &["четверг", "11:00", "11 00"],
            "четверг, 11:00",
            true,
        ),
        probe(
            "S4",
            Stm,
            "Где лежит черновик сметы?",
            &["smeta-v3", "smeta v3", "xlsx"],
            "smeta-v3.xlsx",
            false,
        ),
        probe(
            "S5",
            Stm,
            "Когда можно отправлять смету заказчику?",
            &["пятниц", "не раньше"],
            "не раньше пятницы",
            true,
        ),
        probe(
            "S6",
            Stm,
            "Сколько заложить на инфлюенсеров?",
            &["25 000", "25000", "25 тысяч", "25 тыс"],
            "25 000",
            false,
        ),
        probe(
            "W1",
            Wm,
            "Какая задача сейчас в работе?",
            &[
                "смету маркетинга",
                "смета маркетинга",
                "маркетинга к релизу",
            ],
            "смета маркетинга",
            true,
        ),
        probe(
            "W2",
            Wm,
            "На каком шаге плана мы остановились?",
            &["шаг 2", "шаге 2", "второй шаг"],
            "шаг 2",
            false,
        ),
        probe(
            "W3",
            Wm,
            "Какой вариант сметы выбран рабочим?",
            &["вариант b", "вариант в"],
            "вариант B",
            true,
        ),
        probe(
            "W4",
            Wm,
            "Сколько стоит рабочий вариант?",
            &["140 000", "140000", "140 тысяч", "140 тыс"],
            "140 000",
            false,
        ),
        probe(
            "W5",
            Wm,
            "Какой вопрос остался открытым?",
            &["pr", "пиар", "подрядчик"],
            "PR-подрядчик",
            true,
        ),
        probe(
            "W6",
            Wm,
            "Что уже сделано по задаче?",
            &["каналы собраны", "собраны каналы", "сбор каналов"],
            "каналы собраны",
            false,
        ),
        probe(
            "L1",
            Ltm,
            "Как называется проект?",
            &["атлас"],
            "«Атлас»",
            true,
        ),
        probe(
            "L2",
            Ltm,
            "Как зовут заказчика?",
            &["марина", "ковалев"],
            "Марина Ковалёва",
            false,
        ),
        probe(
            "L3",
            Ltm,
            "Какой дедлайн проекта?",
            &["15 декабря", "15 12 2026", "15.12.2026"],
            "15 декабря 2026",
            true,
        ),
        probe(
            "L4",
            Ltm,
            "Какой общий бюджет проекта?",
            &["480 000", "480000", "480 тысяч", "480 тыс"],
            "480 000",
            false,
        ),
        probe(
            "L5",
            Ltm,
            "Какое ограничение по инфраструктуре?",
            &["российск", "зарубежн", "отечествен"],
            "только российские ЦОД",
            true,
        ),
        probe(
            "L6",
            Ltm,
            "Какую базу данных выбрали?",
            &["postgresql", "постгрес"],
            "PostgreSQL",
            false,
        ),
    ]
}

pub fn quick_probes() -> Vec<Probe> {
    all_probes()
        .into_iter()
        .filter(|probe| probe.quick)
        .collect()
}

pub fn selected_probes(quick: bool) -> Vec<Probe> {
    if quick { quick_probes() } else { all_probes() }
}

/// Оценка ответа по ключевым словам: lower-case, ё → е, пунктуация → пробел.
pub fn probe_matches(probe: &Probe, answer: &str) -> bool {
    let normalized = normalize(answer);
    if normalized.is_empty() {
        return false;
    }
    probe.keywords.iter().any(|keyword| {
        let keyword = normalize(keyword);
        !keyword.is_empty() && normalized.contains(&keyword)
    })
}

pub fn layer_name(layer: MemoryLayer) -> &'static str {
    layer.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_has_18_probes_six_per_layer() {
        let probes = all_probes();
        assert_eq!(probes.len(), 18);
        for layer in MemoryLayer::ALL {
            assert_eq!(
                probes.iter().filter(|probe| probe.layer == layer).count(),
                6
            );
        }
    }

    #[test]
    fn quick_has_9_probes() {
        let probes = quick_probes();
        assert_eq!(probes.len(), 9);
        for layer in MemoryLayer::ALL {
            assert_eq!(
                probes.iter().filter(|probe| probe.layer == layer).count(),
                3
            );
        }
    }

    #[test]
    fn matching_normalizes_case_yo_and_punctuation() {
        let probe = all_probes().into_iter().find(|p| p.id == "L3").unwrap();
        assert!(probe_matches(&probe, "15 ДЕКАБРЯ 2026"));
        assert!(probe_matches(&probe, "15.12.2026"));
        assert!(!probe_matches(&probe, "не знаю"));
    }

    #[test]
    fn matching_accepts_yo_variants() {
        let probe = all_probes().into_iter().find(|p| p.id == "L2").unwrap();
        assert!(probe_matches(&probe, "Марина Ковалева"));
    }

    fn messages_text(from: usize, to: usize) -> String {
        super::super::fixtures::fixture_messages()[from..to]
            .iter()
            .map(|message| message.text())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn stm_probes_are_not_answered_by_phase_a_b() {
        let text = messages_text(0, 26);
        for probe in all_probes()
            .into_iter()
            .filter(|p| p.layer == MemoryLayer::Stm)
        {
            assert!(
                !probe_matches(&probe, &text),
                "{} не должен отвечаться по 1–26",
                probe.id
            );
        }
    }

    #[test]
    fn wm_ltm_probes_are_not_answered_by_phase_c() {
        let text = messages_text(26, 34);
        for probe in all_probes()
            .into_iter()
            .filter(|p| p.layer != MemoryLayer::Stm)
        {
            assert!(
                !probe_matches(&probe, &text),
                "{} не должен отвечаться по 27–34",
                probe.id
            );
        }
    }
}
