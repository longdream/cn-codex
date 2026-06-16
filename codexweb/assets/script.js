// CN-Codex Website JavaScript

document.addEventListener('DOMContentLoaded', function() {
  // Mobile menu toggle
  const mobileMenuBtn = document.querySelector('.mobile-menu-btn');
  const navLinks = document.querySelector('.nav-links');
  
  if (mobileMenuBtn && navLinks) {
    mobileMenuBtn.addEventListener('click', function() {
      navLinks.classList.toggle('open');
      
      // Update aria-expanded
      const isOpen = navLinks.classList.contains('open');
      mobileMenuBtn.setAttribute('aria-expanded', isOpen);
    });
  }
  
  // Docs sidebar navigation - active state on scroll
  const docsNav = document.querySelector('.docs-nav');
  const sections = document.querySelectorAll('.docs-section[id]');
  
  if (docsNav && sections.length > 0) {
    const navLinksItems = docsNav.querySelectorAll('a[href^="#"]');
    
    // Update active link based on scroll position
    function updateActiveLink() {
      const scrollPos = window.scrollY;
      
      sections.forEach(function(section) {
        const sectionTop = section.offsetTop - 100;
        const sectionHeight = section.offsetHeight;
        const sectionId = section.getAttribute('id');
        
        if (scrollPos >= sectionTop && scrollPos < sectionTop + sectionHeight) {
          navLinksItems.forEach(function(link) {
            link.classList.remove('active');
            if (link.getAttribute('href') === '#' + sectionId) {
              link.classList.add('active');
            }
          });
        }
      });
    }
    
    window.addEventListener('scroll', updateActiveLink);
    updateActiveLink(); // Initial call
    
    // Smooth scroll on click
    navLinksItems.forEach(function(link) {
      link.addEventListener('click', function(e) {
        e.preventDefault();
        const targetId = this.getAttribute('href').slice(1);
        const targetSection = document.getElementById(targetId);
        
        if (targetSection) {
          const targetTop = targetSection.offsetTop - 70;
          window.scrollTo({
            top: targetTop,
            behavior: 'smooth'
          });
          
          // Close mobile menu if open
          if (navLinks.classList.contains('open')) {
            navLinks.classList.remove('open');
          }
        }
      });
    });
  }
  
  // Code block copy button (optional enhancement)
  const codeBlocks = document.querySelectorAll('.code-block');
  
  codeBlocks.forEach(function(block) {
    const pre = block.querySelector('pre');
    const code = pre ? pre.querySelector('code') : null;
    
    if (code) {
      // Add copy button
      const copyBtn = document.createElement('button');
      copyBtn.className = 'copy-btn';
      copyBtn.textContent = '复制';
      copyBtn.style.cssText = 'position: absolute; top: 8px; right: 8px; background: var(--surface-soft); border: 1px solid var(--border-subtle); color: var(--text-muted); padding: 4px 8px; border-radius: 4px; font-size: 12px; cursor: pointer;';
      
      // Make code block container relative
      pre.style.position = 'relative';
      pre.appendChild(copyBtn);
      
      copyBtn.addEventListener('click', function() {
        const text = code.textContent;
        
        navigator.clipboard.writeText(text).then(function() {
          copyBtn.textContent = '已复制';
          copyBtn.style.color = 'var(--accent)';
          
          setTimeout(function() {
            copyBtn.textContent = '复制';
            copyBtn.style.color = 'var(--text-muted)';
          }, 2000);
        }).catch(function() {
          // Fallback for older browsers
          const textarea = document.createElement('textarea');
          textarea.value = text;
          textarea.style.position = 'fixed';
          textarea.style.opacity = '0';
          document.body.appendChild(textarea);
          textarea.select();
          document.execCommand('copy');
          document.body.removeChild(textarea);
          
          copyBtn.textContent = '已复制';
          copyBtn.style.color = 'var(--accent)';
          
          setTimeout(function() {
            copyBtn.textContent = '复制';
            copyBtn.style.color = 'var(--text-muted)';
          }, 2000);
        });
      });
    }
  });
  
  // Animate elements on scroll (optional)
  const animateElements = document.querySelectorAll('.feature-card, .plugin-card, .tool-category');
  
  if (animateElements.length > 0 && 'IntersectionObserver' in window) {
    const observer = new IntersectionObserver(function(entries) {
      entries.forEach(function(entry) {
        if (entry.isIntersecting) {
          entry.target.style.opacity = '1';
          entry.target.style.transform = 'translateY(0)';
          observer.unobserve(entry.target);
        }
      });
    }, {
      threshold: 0.1,
      rootMargin: '0px 0px -50px 0px'
    });
    
    animateElements.forEach(function(el) {
      el.style.opacity = '0';
      el.style.transform = 'translateY(20px)';
      el.style.transition = 'opacity 0.4s ease, transform 0.4s ease';
      observer.observe(el);
    });
  }
});

// Smooth scroll to anchor from other pages
window.addEventListener('load', function() {
  const hash = window.location.hash;
  
  if (hash) {
    const target = document.getElementById(hash.slice(1));
    
    if (target) {
      setTimeout(function() {
        const targetTop = target.offsetTop - 70;
        window.scrollTo({
          top: targetTop,
          behavior: 'smooth'
        });
      }, 100);
    }
  }
});