import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import zh from "./locales/zh.json";

const lng = navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";

void i18next.use(initReactI18next).init({
  resources: { zh: { translation: zh }, en: { translation: en } },
  lng,
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

export default i18next;
