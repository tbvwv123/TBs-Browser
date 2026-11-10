import { mount } from "svelte";
import "./styles/base.css";
import App from "./App.svelte";
import Popup from "./lib/Popup.svelte";

// 같은 번들을 두 곳에서 쓴다: 브라우저 화면(UI 웹뷰)과 콘텐츠 위에 뜨는 팝업 창
const target = document.getElementById("app")!;
if (location.hash === "#popup") mount(Popup, { target });
else mount(App, { target });
