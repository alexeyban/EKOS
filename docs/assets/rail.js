(function(){
  var links = Array.prototype.slice.call(document.querySelectorAll('.rail a'));
  var sections = links.map(function(a){ return document.querySelector(a.getAttribute('href')); });
  if(!('IntersectionObserver' in window)) return;
  var io = new IntersectionObserver(function(entries){
    entries.forEach(function(e){
      if(e.isIntersecting){
        var idx = sections.indexOf(e.target);
        links.forEach(function(l,i){ l.classList.toggle('current', i===idx); });
      }
    });
  }, { threshold: 0.55 });
  sections.forEach(function(s){ if(s) io.observe(s); });
})();
